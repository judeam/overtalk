//! Private parent/child transport. No listener, URL, shell, or evaluation.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Read, Write};

pub const PREFIX: &[u8] = b"OTRPC1 ";
pub const MAX_REQUEST: usize = 1024 * 1024;
pub const MAX_RESPONSE: usize = 16 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: u64,
    pub command: String,
    #[serde(default)]
    pub args: Value,
}

impl Request {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id == 0 || self.command.is_empty() || self.command.len() > 100 {
            return Err("Invalid request identifier or command");
        }
        if !self
            .command
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err("Invalid command name");
        }
        if !self.args.is_null() && !self.args.is_object() {
            return Err("Command arguments must be an object");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Message {
    Ready {
        commands: Vec<String>,
    },
    Reply {
        id: u64,
        result: Value,
        error: Option<String>,
    },
    Event {
        event: String,
        payload: Value,
    },
}

pub fn write_frame(
    writer: &mut impl Write,
    value: &impl Serialize,
    limit: usize,
) -> io::Result<()> {
    struct Bounded {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "RPC frame too large",
                ));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Bounded {
        bytes: Vec::with_capacity(limit.min(4096)),
        limit,
    };
    serde_json::to_writer(&mut buffer, value).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "RPC frame too large or invalid")
    })?;
    let bytes = buffer.bytes;
    writer.write_all(PREFIX)?;
    writer.write_all(&bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Also bounds non-protocol lines produced by a native dependency on stdout.
pub fn read_frame(reader: &mut impl BufRead, limit: usize) -> io::Result<Option<Vec<u8>>> {
    loop {
        let mut line = Vec::with_capacity(4096);
        let length = (&mut *reader)
            .take((limit + PREFIX.len() + 2) as u64)
            .read_until(b'\n', &mut line)?;
        if length == 0 {
            return Ok(None);
        }
        if line.last() != Some(&b'\n') || length > limit + PREFIX.len() + 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "RPC frame too large or truncated",
            ));
        }
        if line.starts_with(PREFIX) {
            line.drain(..PREFIX.len());
            line.pop();
            return Ok(Some(line));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn bounded_frames_round_trip_around_non_protocol_output() {
        let request = Request {
            id: 1,
            command: "api_get_meetings".into(),
            args: Value::Null,
        };
        let mut bytes = b"native startup message\n".to_vec();
        write_frame(&mut bytes, &request, MAX_REQUEST).unwrap();
        let result: Request = serde_json::from_slice(
            &read_frame(&mut Cursor::new(bytes), MAX_REQUEST)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result.command, request.command);
        result.validate().unwrap();
    }

    #[test]
    fn oversized_truncated_or_malformed_input_is_rejected() {
        let mut output = Vec::new();
        assert!(write_frame(&mut output, &"x".repeat(1000), 20).is_err());
        assert!(
            output.is_empty(),
            "An oversized frame must not partially reach the pipe"
        );
        for data in [b"OTRPC1 unfinished".to_vec(), vec![b'x'; 100]] {
            assert!(read_frame(&mut Cursor::new(data), 20).is_err());
        }
        assert!(Request {
            id: 0,
            command: "x".into(),
            args: Value::Null
        }
        .validate()
        .is_err());
        assert!(Request {
            id: 1,
            command: "../../bin/sh".into(),
            args: Value::Null
        }
        .validate()
        .is_err());
        assert!(Request {
            id: 1,
            command: "read".into(),
            args: Value::String("code".into())
        }
        .validate()
        .is_err());
    }
}
