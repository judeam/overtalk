//! The UI never performs database, model, network or audio work on its event thread.
use overtalk_protocol::{read_frame, write_frame, Message, Request, MAX_REQUEST, MAX_RESPONSE};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::BufReader,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

type Completion = Box<dyn FnOnce(Result<Value, String>) + Send>;
type Pending = Arc<Mutex<HashMap<u64, Completion>>>;

#[derive(Clone)]
pub(crate) struct Engine {
    sender: mpsc::SyncSender<Request>,
    pending: Pending,
    sequence: Arc<AtomicU64>,
    child: Arc<Mutex<Child>>,
}

impl Engine {
    pub(crate) fn start(
        path: &Path,
        on_event: impl Fn(Message) + Send + 'static,
    ) -> Result<Self, String> {
        if !path.is_file() {
            return Err(format!("The meeting engine is missing: {}", path.display()));
        }
        let mut command = Command::new(path);
        command
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Could not start the meeting engine: {e}"))?;
        let mut input = child.stdin.take().ok_or("Missing engine input")?;
        let output = child.stdout.take().ok_or("Missing engine output")?;
        let (sender, requests) = mpsc::sync_channel::<Request>(64);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let writer_pending = pending.clone();
        std::thread::spawn(move || {
            for request in requests {
                if let Err(error) = write_frame(&mut input, &request, MAX_REQUEST) {
                    // Serialization/size validation finishes before write_frame
                    // touches the pipe. Reject only this request; later calls
                    // can still use the intact connection.
                    if error.kind() == std::io::ErrorKind::InvalidData {
                        let completion = writer_pending.lock().unwrap().remove(&request.id);
                        if let Some(completion) = completion {
                            completion(Err(
                                "This request exceeds the 1 MiB limit. Reduce its size and try again."
                                    .into(),
                            ));
                        }
                        continue;
                    }
                    fail_pending(
                        &writer_pending,
                        &format!("Meeting engine connection closed: {error}"),
                    );
                    break;
                }
            }
        });
        let reader_pending = pending.clone();
        std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            while let Ok(Some(bytes)) = read_frame(&mut output, MAX_RESPONSE) {
                let message: Message = match serde_json::from_slice(&bytes) {
                    Ok(message) => message,
                    Err(_) => break,
                };
                match message {
                    Message::Reply { id, result, error } => {
                        let completion = reader_pending.lock().unwrap().remove(&id);
                        if let Some(completion) = completion {
                            completion(error.map_or(Ok(result), Err));
                        }
                    }
                    message => on_event(message),
                }
            }
            fail_pending(
                &reader_pending,
                "The meeting engine stopped. Close and reopen Overtalk to reconnect.",
            );
            on_event(Message::Event {
                event: "engine-disconnected".into(),
                payload: Value::Null,
            });
        });
        Ok(Self {
            sender,
            pending,
            sequence: Arc::new(AtomicU64::new(1)),
            child: Arc::new(Mutex::new(child)),
        })
    }

    pub(crate) fn call(
        &self,
        command: &str,
        args: Value,
        completion: impl FnOnce(Result<Value, String>) + Send + 'static,
    ) {
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        let mut pending = self.pending.lock().unwrap();
        if pending.len() >= 64 {
            drop(pending);
            completion(Err("The meeting engine is busy. Try again shortly.".into()));
            return;
        }
        pending.insert(id, Box::new(completion));
        drop(pending);
        if self
            .sender
            .try_send(Request {
                id,
                command: command.into(),
                args,
            })
            .is_err()
        {
            let completion = self.pending.lock().unwrap().remove(&id);
            if let Some(completion) = completion {
                completion(Err("The meeting engine is unavailable".into()));
            }
        }
    }

    /// Called after the native window closes, outside the UI event loop.
    pub(crate) fn shutdown(&self) {
        let _ = self.sender.try_send(Request {
            id: u64::MAX,
            command: "shutdown".into(),
            args: Value::Null,
        });
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let mut child = self.child.lock().unwrap();
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            drop(child);
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn fail_pending(pending: &Pending, error: &str) {
    let completions = std::mem::take(&mut *pending.lock().unwrap());
    for (_, completion) in completions {
        completion(Err(error.into()));
    }
}

/// On macOS the engine inherits the app's microphone and screen-recording permissions, so
/// an installed app bundle never runs an engine named by `OVERTALK_SERVICE`.
fn honours_service_override(macos: bool, exe: Option<&Path>) -> bool {
    !macos || !exe.is_some_and(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS/"))
}

pub(crate) fn service_path() -> PathBuf {
    let exe = std::env::current_exe().ok();
    if let Some(path) = std::env::var_os("OVERTALK_SERVICE")
        .filter(|_| honours_service_override(cfg!(target_os = "macos"), exe.as_deref()))
    {
        return path.into();
    }
    let name = if cfg!(windows) {
        "sonefold-service.exe"
    } else {
        "sonefold-service"
    };
    let adjacent = exe.and_then(|path| path.parent().map(|parent| parent.join(name)));
    // A development build falls back to this checkout's release engine; a
    // release build looks only beside itself, so no build path is compiled in.
    #[cfg(debug_assertions)]
    {
        if let Some(path) = adjacent.filter(|path| path.is_file()) {
            return path;
        }
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/release")
            .join(name)
    }
    #[cfg(not(debug_assertions))]
    adjacent.unwrap_or_else(|| PathBuf::from(name))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn an_installed_mac_app_ignores_the_engine_override() {
        let bundle = Path::new("/Applications/Overtalk.app/Contents/MacOS/overtalk");
        let checkout = Path::new("/src/overtalk/target/release/examples/visual_review");
        assert!(!honours_service_override(true, Some(bundle)));
        assert!(honours_service_override(true, Some(checkout)));
        assert!(honours_service_override(false, Some(bundle)));
    }

    #[test]
    fn oversized_request_does_not_disconnect_or_fail_other_requests() {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("synthetic-service");
        std::fs::write(&script, r#"#!/usr/bin/env python3
import json,sys
print('OTRPC1 {"kind":"ready","commands":[]}',flush=True)
count=0
for line in sys.stdin:
    request=json.loads(line[7:])
    if request['command']=='shutdown': break
    count+=1
    print('OTRPC1 '+json.dumps(dict(kind='reply',id=request['id'],result=count,error=None)),flush=True)
"#).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (events, ready) = mpsc::channel();
        let engine = Engine::start(&script, move |event| {
            let _ = events.send(event);
        })
        .unwrap();
        assert!(matches!(
            ready.recv_timeout(Duration::from_secs(5)).unwrap(),
            Message::Ready { .. }
        ));
        let (large_send, large_receive) = mpsc::channel();
        let (small_send, small_receive) = mpsc::channel();
        engine.call(
            "save",
            serde_json::json!({"body":"x".repeat(MAX_REQUEST)}),
            move |result| {
                let _ = large_send.send(result);
            },
        );
        engine.call("read", Value::Null, move |result| {
            let _ = small_send.send(result);
        });
        let large = large_receive.recv_timeout(Duration::from_secs(5));
        let small = small_receive.recv_timeout(Duration::from_secs(5));
        engine.shutdown();
        assert!(large.unwrap().is_err());
        assert_eq!(small.unwrap().unwrap(), serde_json::json!(1));
    }
}
