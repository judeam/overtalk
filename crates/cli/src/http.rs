//! The running app's local API: one request per connection to 127.0.0.1,
//! never through a proxy, never anywhere else. The app proves it holds this
//! client's token before the token is sent, so a program that took the port
//! while Overtalk was closed never receives it.
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::{json, Value};

const MAX_REQUEST: usize = 1024 * 1024;
/// Transcripts and notes can be large; this bounds a reply, not a meeting.
const MAX_RESPONSE: u64 = 64 * 1024 * 1024;

pub(crate) struct App {
    pub port: u16,
    pub token: String,
    /// Which client's token this is ("cli" or "mcp"), so the app proves the right one.
    pub client: &'static str,
}

#[derive(Debug, PartialEq)]
pub(crate) enum CallError {
    /// Nothing is listening: the app is closed or local access is off.
    NotRunning,
    /// The app answered with an error, such as a missing grant.
    Failed(String),
}

impl CallError {
    pub fn message(self) -> String {
        match self {
            CallError::NotRunning => "Overtalk is not running. Start it, then try again.".into(),
            CallError::Failed(message) => message,
        }
    }
}

impl App {
    pub fn call(&self, name: &str, arguments: &Value) -> Result<Value, CallError> {
        let body = json!({"name": name, "arguments": arguments}).to_string();
        if body.len() > MAX_REQUEST {
            return Err(CallError::Failed("The request exceeds 1 MiB".into()));
        }
        self.check_identity()?;
        let request = format!(
            "POST /v1/call HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.port,
            self.token,
            body.len()
        );
        let (status, body) = self.exchange(&request)?;
        let payload: Value = serde_json::from_slice(&body)
            .map_err(|_| CallError::Failed("Overtalk sent an unreadable reply".into()))?;
        if status == 200 {
            return Ok(payload["result"].clone());
        }
        Err(CallError::Failed(
            payload["error"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Overtalk answered with HTTP {status}")),
        ))
    }

    /// Ask whoever listens on the port to prove it holds this client's token.
    fn check_identity(&self) -> Result<(), CallError> {
        let mut bytes = [0u8; 16];
        getrandom::getrandom(&mut bytes)
            .map_err(|_| CallError::Failed("Could not create a random check".into()))?;
        let nonce: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let request = format!(
            "GET /v1/proof HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Overtalk-Client: {}\r\n\
             X-Overtalk-Nonce: {nonce}\r\nConnection: close\r\n\r\n",
            self.port, self.client
        );
        let (status, body) = self.exchange(&request)?;
        let answer = serde_json::from_slice::<Value>(&body).ok();
        let proof = answer.as_ref().and_then(|v| v["proof"].as_str());
        match proof {
            Some(proof)
                if status == 200
                    && overtalk_library_reads::proof::verify(&self.token, self.port, &nonce, proof) =>
            {
                Ok(())
            }
            _ if status == 404 => Err(CallError::Failed(
                "Overtalk did not recognise this client. Turn it on and select Save in Settings, then try again.".into(),
            )),
            _ => Err(CallError::Failed(format!(
                "Something other than Overtalk is answering on port {}; the access token was not sent.",
                self.port
            ))),
        }
    }

    /// One request and its reply over a fresh loopback connection.
    fn exchange(&self, request: &str) -> Result<(u16, Vec<u8>), CallError> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
            .map_err(|_| CallError::NotRunning)?;
        let failed = |error: std::io::Error| {
            CallError::Failed(format!("The connection to Overtalk failed: {error}"))
        };
        // Summaries and recordings can take a while to start; reads answer at once.
        stream
            .set_read_timeout(Some(Duration::from_secs(600)))
            .map_err(failed)?;
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(failed)?;
        stream.write_all(request.as_bytes()).map_err(failed)?;
        let mut raw = Vec::new();
        stream
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut raw)
            .map_err(failed)?;
        if raw.len() as u64 > MAX_RESPONSE {
            return Err(CallError::Failed("Overtalk's reply is too large".into()));
        }
        response(&raw).map_err(CallError::Failed)
    }
}

/// Status and body of an HTTP/1.1 response, with or without chunked encoding.
fn response(raw: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let unreadable = || "Overtalk sent an unreadable reply".to_owned();
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(unreadable)?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| unreadable())?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(unreadable)?;
    let mut length = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
        if name == "content-length" {
            length = Some(value.parse::<usize>().map_err(|_| unreadable())?);
        } else if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        }
    }
    let body = &raw[split + 4..];
    if chunked {
        return dechunk(body)
            .map(|body| (status, body))
            .ok_or_else(unreadable);
    }
    match length {
        Some(length) if length <= body.len() => Ok((status, body[..length].to_vec())),
        Some(_) => Err(unreadable()),
        None => Ok((status, body.to_vec())),
    }
}

fn dechunk(mut body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let end = body.windows(2).position(|window| window == b"\r\n")?;
        let size = std::str::from_utf8(&body[..end]).ok()?;
        let size = usize::from_str_radix(size.split(';').next()?.trim(), 16).ok()?;
        body = &body[end + 2..];
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(body.get(..size)?);
        body = body.get(size + 2..)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    #[test]
    fn replies_are_read_with_a_length_or_in_chunks() {
        let plain = b"HTTP/1.1 200 OK\r\ncontent-length: 11\r\n\r\n{\"a\":true}\nextra";
        assert_eq!(response(plain).unwrap(), (200, b"{\"a\":true}\n".to_vec()));
        let chunked = b"HTTP/1.1 400 Bad Request\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"e\"\r\n5\r\n:\"x\"}\r\n0\r\n\r\n";
        assert_eq!(response(chunked).unwrap(), (400, b"{\"e\":\"x\"}".to_vec()));
        assert!(response(b"HTTP/1.1 200 OK\r\ncontent-length: 99\r\n\r\n{}").is_err());
        assert!(response(b"garbage").is_err());
    }

    /// Read one request from a connection: its head and body.
    fn read_request(
        stream: std::net::TcpStream,
    ) -> (String, std::io::BufReader<std::net::TcpStream>) {
        let mut reader = std::io::BufReader::new(stream);
        let mut head = String::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
            head.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        (head + &String::from_utf8(body).unwrap(), reader)
    }

    fn header<'a>(request: &'a str, name: &str) -> &'a str {
        request
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .unwrap_or("")
            .trim()
    }

    /// A stand-in for the app's listener: it proves `token`, then answers one call
    /// with `reply`. Returns every request it received.
    fn serve(
        token: &'static str,
        reply: &'static str,
    ) -> (u16, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let (check, mut reader) = read_request(stream);
            let nonce = header(&check, "X-Overtalk-Nonce:").to_owned();
            let body = json!({"proof": overtalk_library_reads::proof::proof(token, port, &nonce)})
                .to_string();
            let answer = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            );
            reader.get_mut().write_all(answer.as_bytes()).unwrap();
            drop(reader);
            let (stream, _) = listener.accept().unwrap();
            let (call, mut reader) = read_request(stream);
            reader.get_mut().write_all(reply.as_bytes()).unwrap();
            vec![check, call]
        });
        (port, handle)
    }

    fn app(port: u16) -> App {
        App {
            port,
            token: "t".repeat(64),
            client: "cli",
        }
    }

    #[test]
    fn calls_carry_the_client_token_to_loopback_and_return_the_result() {
        let token: &'static str = Box::leak("t".repeat(64).into_boxed_str());
        let (port, server) = serve(
            token,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 25\r\n\r\n{\"result\":{\"started\":1}}\n",
        );
        let result = app(port)
            .call("start_recording", &json!({"title":"Standup"}))
            .unwrap();
        assert_eq!(result, json!({"started":1}));
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /v1/proof HTTP/1.1\r\n"));
        assert_eq!(header(&requests[0], "X-Overtalk-Client:"), "cli");
        assert!(!requests[0].contains("Authorization"));
        let request = &requests[1];
        assert!(request.starts_with("POST /v1/call HTTP/1.1\r\n"));
        assert!(request.contains(&format!("Host: 127.0.0.1:{port}\r\n")));
        assert!(request.contains(&format!("Authorization: Bearer {}\r\n", "t".repeat(64))));
        assert!(request.ends_with(r#"{"arguments":{"title":"Standup"},"name":"start_recording"}"#));
    }

    #[test]
    fn a_listener_that_cannot_prove_the_token_never_receives_it() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let squatter = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let (request, mut reader) = read_request(stream);
            let body = json!({"proof": "0".repeat(64)}).to_string();
            let answer = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            );
            reader.get_mut().write_all(answer.as_bytes()).unwrap();
            listener.set_nonblocking(true).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(200));
            // No second connection carrying the token arrives.
            (request, listener.accept().is_err())
        });
        let error = app(port).call("stop_recording", &json!({})).unwrap_err();
        assert!(error.message().contains("the access token was not sent"));
        let (request, no_second) = squatter.join().unwrap();
        assert!(!request.contains(&"t".repeat(64)) && no_second);
    }

    #[test]
    fn app_errors_and_a_closed_app_are_told_apart() {
        let token: &'static str = Box::leak("t".repeat(64).into_boxed_str());
        let (port, server) = serve(
            token,
            "HTTP/1.1 400 Bad Request\r\ncontent-length: 50\r\n\r\n{\"error\":\"Recording control is disabled in Integ\"}",
        );
        assert_eq!(
            app(port).call("stop_recording", &json!({})),
            Err(CallError::Failed(
                "Recording control is disabled in Integ".into()
            ))
        );
        server.join().unwrap();
        let closed = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        assert_eq!(
            app(port).call("stop_recording", &json!({})),
            Err(CallError::NotRunning)
        );
    }
}
