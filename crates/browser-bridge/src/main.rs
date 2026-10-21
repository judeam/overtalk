//! Browser native messaging, with no listening port and no arbitrary commands.
use std::io::{BufWriter, ErrorKind, Read, Write};
fn allowed(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    let host = url.host_str().unwrap_or("");
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && (host == "meet.google.com"
            || host == "zoom.us"
            || host.ends_with(".zoom.us")
            || host == "teams.microsoft.com"
            || host == "teams.live.com"
            || host.ends_with(".webex.com"))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    loop {
        let mut length = [0; 4];
        // Only EOF between frames is clean. A partial length must be reported
        // as truncated rather than silently looking like successful shutdown.
        match input.read_exact(&mut length[..1]) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error.into()),
        }
        input.read_exact(&mut length[1..])?;
        let size = u32::from_le_bytes(length) as usize;
        if size == 0 || size > 8192 {
            return Err("Native message too large".into());
        }
        let mut bytes = vec![0; size];
        input.read_exact(&mut bytes)?;
        let request: serde_json::Value = serde_json::from_slice(&bytes)?;
        let raw = request["url"].as_str().unwrap_or("");
        if !allowed(raw) {
            return Err("Unsupported meeting origin".into());
        }
        let directory = dirs::data_dir()
            .ok_or("No application data directory")?
            .join("com.judeam.sonefold");
        std::fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        }
        let mut file = tempfile::NamedTempFile::new_in(&directory)?;
        {
            // Serialize into one buffered write, not a syscall per JSON token.
            // Flush failures must propagate before the atomic publication.
            let mut output = BufWriter::new(file.as_file_mut());
            serde_json::to_writer(
                &mut output,
                &serde_json::json!({"url":raw,"title":request["title"].as_str().unwrap_or("Meeting page").chars().take(240).collect::<String>(),"active":request["active"]==true,"seen_at":chrono::Utc::now().timestamp()}),
            )?;
            output.flush()?;
        }
        file.as_file().sync_all()?;
        file.persist(directory.join("browser-meeting.json"))?;
        let reply = b"{\"ok\":true}";
        output.write_all(&(reply.len() as u32).to_le_bytes())?;
        output.write_all(reply)?;
        output.flush()?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restrict_origins() {
        assert!(allowed("https://meet.google.com/abc-defg-hij"));
        assert!(allowed("https://tenant.zoom.us/j/123"));
        for bad in [
            "https://meet.google.com.evil.com/",
            "http://meet.google.com/",
            "file:///etc/passwd",
            "https://u:p@zoom.us/",
            "https://example.com",
        ] {
            assert!(!allowed(bad));
        }
    }
}
