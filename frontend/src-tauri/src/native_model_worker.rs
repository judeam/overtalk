//! Persistent, bounded binary-PCM protocol for native inference. The service
//! never loads Whisper/ONNX parsers; children receive only model files and PCM.
use anyhow::{anyhow, Result};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Stdio},
    sync::atomic::{AtomicBool, Ordering},
};
static IS_WORKER: AtomicBool = AtomicBool::new(false);
pub(crate) fn is_worker() -> bool {
    IS_WORKER.load(Ordering::Relaxed)
}
const MAX_SAMPLES: usize = 16000 * 60 * 30;
const MAX_RESPONSE: usize = 8 * 1024 * 1024;

pub(crate) struct NativeModelWorker {
    child: std::sync::Arc<std::sync::Mutex<Child>>,
    stdin: ChildStdin,
    responses: std::sync::mpsc::Receiver<Result<Vec<u8>>>,
    sandbox: Option<overtalk_worker_sandbox::Sandbox>,
}
fn line<R: BufRead>(reader: &mut R, max: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(max as u64 + 1).read_until(b'\n', &mut bytes)?;
    if bytes.is_empty() || bytes.len() > max || bytes.last() != Some(&b'\n') {
        return Err(anyhow!("Invalid or oversized native worker response"));
    }
    Ok(bytes)
}
impl NativeModelWorker {
    pub(crate) fn launch(kind: &str, directory: &Path, model: &str, gpu: bool) -> Result<Self> {
        let exe = std::env::current_exe()?;
        let filename = if cfg!(windows) {
            "sonefold-service.exe"
        } else {
            "sonefold-service"
        };
        let service = [
            exe.parent().unwrap().join(filename),
            exe.parent().unwrap().join("..").join(filename),
        ]
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| anyhow!("Native inference worker is missing; reinstall Overtalk"))?;
        let mut sandbox = overtalk_worker_sandbox::Sandbox::new(gpu)?;
        sandbox.read(directory)?;
        let wrapper = overtalk_worker_sandbox::gpu_launcher().filter(|_| gpu);
        let mut command = if let Some(wrapper) = wrapper {
            sandbox.command_with_wrapper(&service, &wrapper)?
        } else {
            sandbox.command(&service)?
        };
        command
            .args(["--native-model-worker", kind])
            .arg(directory)
            .arg(model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Missing worker input"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Missing worker output"))?;
        let (sender, responses) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let response = line(&mut reader, MAX_RESPONSE);
                let finished = response.is_err();
                if sender.send(response).is_err() || finished {
                    break;
                }
            }
        });
        let mut worker = Self {
            child: std::sync::Arc::new(std::sync::Mutex::new(child)),
            stdin,
            responses,
            sandbox: Some(sandbox),
        };
        let reply: Value =
            serde_json::from_slice(&worker.response(std::time::Duration::from_secs(180))?)?;
        if reply["ready"] != true {
            return Err(anyhow!("Native model could not be initialized"));
        }
        Ok(worker)
    }
    fn response(&mut self, deadline: std::time::Duration) -> Result<Vec<u8>> {
        match self.responses.recv_timeout(deadline) {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(error)) => {
                let _ = self
                    .child
                    .lock()
                    .map(|mut child| overtalk_worker_sandbox::terminate(&mut child));
                Err(error)
            }
            Err(_) => {
                let _ = self
                    .child
                    .lock()
                    .map(|mut child| overtalk_worker_sandbox::terminate(&mut child));
                Err(anyhow!(
                    "Native inference worker stopped or exceeded its deadline"
                ))
            }
        }
    }
    pub(crate) fn call<T: DeserializeOwned>(
        &mut self,
        operation: &str,
        samples: &[f32],
        language: Option<String>,
    ) -> Result<T> {
        let _deadline = Deadline::new(self.child.clone(), std::time::Duration::from_secs(600));
        if samples.len() > MAX_SAMPLES || samples.iter().any(|x| !x.is_finite()) {
            return Err(anyhow!("Invalid or oversized inference input"));
        }
        let header = json!({"operation":operation, "samples":samples.len(), "language":language});
        serde_json::to_writer(&mut self.stdin, &header)?;
        self.stdin.write_all(b"\n")?;
        // A fixed scratch block avoids another full-meeting allocation.
        let mut scratch = [0u8; 16384];
        for chunk in samples.chunks(scratch.len() / 4) {
            for (sample, bytes) in chunk.iter().zip(scratch.chunks_exact_mut(4)) {
                bytes.copy_from_slice(&sample.to_le_bytes());
            }
            self.stdin.write_all(&scratch[..chunk.len() * 4])?;
        }
        self.stdin.flush()?;
        let reply: Value =
            serde_json::from_slice(&self.response(std::time::Duration::from_secs(600))?)?;
        if reply.get("error").is_some() {
            return Err(anyhow!("Native inference failed"));
        }
        Ok(serde_json::from_value(reply["result"].clone())?)
    }
}
impl Drop for NativeModelWorker {
    fn drop(&mut self) {
        let _ = self
            .child
            .lock()
            .map(|mut child| overtalk_worker_sandbox::terminate(&mut child));
        // Wait releases the process and mapping before cleaning its private files.
        let _ = self.child.lock().map(|mut child| child.wait());
        self.sandbox.take();
    }
}
/// Covers blocked writes as well as reads: a compromised worker cannot keep a
/// service thread forever by sending a fake reply then refusing further input.
struct Deadline(std::sync::mpsc::Sender<()>);
impl Deadline {
    fn new(child: std::sync::Arc<std::sync::Mutex<Child>>, duration: std::time::Duration) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if matches!(
                receiver.recv_timeout(duration),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                if let Ok(mut child) = child.lock() {
                    overtalk_worker_sandbox::terminate(&mut child);
                }
            }
        });
        Self(sender)
    }
}
impl Drop for Deadline {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[derive(Deserialize)]
struct Request {
    operation: String,
    samples: usize,
    language: Option<String>,
}
enum Model {
    Whisper(crate::whisper_engine::WhisperEngine),
    Parakeet(crate::parakeet_engine::ParakeetEngine),
    Voice(overtalk_voice_memory::Encoder),
}
/// Dedicated entry point: no database, GUI, network service or credentials.
pub fn run(args: &[std::ffi::OsString]) -> Result<()> {
    IS_WORKER.store(true, Ordering::Relaxed);
    if args.len() != 3 {
        return Err(anyhow!("Invalid native inference arguments"));
    }
    let directory = PathBuf::from(&args[1]);
    let name = args[2]
        .to_str()
        .ok_or_else(|| anyhow!("Invalid model name"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut model = runtime.block_on(async {
        Ok::<_, anyhow::Error>(match args[0].to_str() {
            Some("whisper") => {
                let engine =
                    crate::whisper_engine::WhisperEngine::new_with_models_dir(Some(directory))?;
                engine.discover_models().await?;
                engine.load_model(name).await?;
                Model::Whisper(engine)
            }
            Some("parakeet") => {
                let root = directory
                    .parent()
                    .ok_or_else(|| anyhow!("Invalid model directory"))?
                    .to_owned();
                let engine =
                    crate::parakeet_engine::ParakeetEngine::new_with_models_dir(Some(root))?;
                engine.discover_models().await?;
                engine.load_model(name).await?;
                Model::Parakeet(engine)
            }
            Some("voice") => Model::Voice(
                overtalk_voice_memory::Encoder::open(&directory.join(name))
                    .map_err(|e| anyhow!(e))?,
            ),
            _ => return Err(anyhow!("Unknown native inference model")),
        })
    })?;
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut output = std::io::stdout().lock();
    writeln!(output, "{{\"ready\":true}}")?;
    output.flush()?;
    loop {
        if input.fill_buf()?.is_empty() {
            break;
        }
        let request: Request = serde_json::from_slice(&line(&mut input, 65536)?)?;
        if request.samples > MAX_SAMPLES {
            return Err(anyhow!("Oversized inference input"));
        }
        let mut bytes = vec![0u8; request.samples * 4];
        input.read_exact(&mut bytes)?;
        let samples: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        drop(bytes);
        if samples.iter().any(|x| !x.is_finite()) {
            return Err(anyhow!("Invalid samples"));
        }
        let result: Result<Value> = runtime.block_on(async {
            Ok(match &mut model {
                Model::Whisper(engine) if request.operation == "confidence" => {
                    serde_json::to_value(
                        engine
                            .transcribe_audio_with_confidence(samples, request.language)
                            .await?,
                    )?
                }
                Model::Whisper(engine) if request.operation == "transcribe" => {
                    serde_json::to_value(engine.transcribe_audio(samples, request.language).await?)?
                }
                Model::Parakeet(engine) if request.operation == "transcribe" => {
                    serde_json::to_value(engine.transcribe_audio_timestamped(samples).await?)?
                }
                Model::Voice(encoder) if request.operation == "encode" => {
                    serde_json::to_value(encoder.encode(&samples).map_err(|e| anyhow!(e))?)?
                }
                _ => return Err(anyhow!("Unknown inference operation")),
            })
        });
        let response = match result {
            Ok(value) => json!({"result":value}),
            Err(_) => json!({"error":"Inference failed"}),
        };
        let bytes = serde_json::to_vec(&response)?;
        if bytes.len() > MAX_RESPONSE {
            return Err(anyhow!("Oversized inference result"));
        }
        output.write_all(&bytes)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

pub(crate) type Shared = std::sync::Arc<std::sync::Mutex<Option<NativeModelWorker>>>;
pub(crate) fn shared() -> Shared {
    std::sync::Arc::new(std::sync::Mutex::new(None))
}
pub(crate) async fn load_shared(
    worker: Shared,
    kind: &'static str,
    directory: PathBuf,
    name: String,
    gpu: bool,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut slot = worker
            .lock()
            .map_err(|_| anyhow!("Inference lock failed"))?;
        slot.take();
        *slot = Some(NativeModelWorker::launch(kind, &directory, &name, gpu)?);
        Ok(())
    })
    .await?
}
pub(crate) async fn call_shared<T: DeserializeOwned + Send + 'static>(
    worker: Shared,
    operation: &'static str,
    samples: Vec<f32>,
    language: Option<String>,
) -> Result<T> {
    tokio::task::spawn_blocking(move || {
        worker
            .lock()
            .map_err(|_| anyhow!("Inference lock failed"))?
            .as_mut()
            .ok_or_else(|| anyhow!("No native model is loaded"))?
            .call(operation, &samples, language)
    })
    .await?
}
pub(crate) async fn unload_shared(worker: Shared) -> bool {
    tokio::task::spawn_blocking(move || {
        worker
            .lock()
            .map(|mut w| w.take().is_some())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}
pub(crate) fn loaded(worker: &Shared) -> bool {
    match worker.try_lock() {
        Ok(w) => w.as_ref().is_some_and(|worker| {
            worker
                .child
                .lock()
                .is_ok_and(|mut child| matches!(child.try_wait(), Ok(None)))
        }),
        Err(std::sync::TryLockError::WouldBlock) => true,
        Err(_) => false,
    }
}
