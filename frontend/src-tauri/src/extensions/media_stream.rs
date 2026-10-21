//! Linux WebKit cannot reliably stream media through Tauri's custom file scheme.
//! Give the player a revocable capability for one already-open meeting recording.
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use std::{
    collections::HashMap,
    convert::Infallible,
    fs::File,
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{OnceCell, Semaphore};

static SERVICE: OnceCell<MediaService> = OnceCell::const_new();
struct Recording {
    file: File,
    length: u64,
    content_type: &'static str,
    touched: Instant,
    revoked: bool,
    meeting_id: String,
}
struct MediaService {
    address: SocketAddr,
    recordings: Arc<Mutex<HashMap<String, Arc<Mutex<Recording>>>>>,
    #[cfg(test)]
    streams: Arc<Semaphore>,
}
const TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_RECORDINGS: usize = 16;

impl MediaService {
    async fn start() -> Result<Self, String> {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let recordings = Arc::new(Mutex::new(HashMap::new()));
        let streams = Arc::new(Semaphore::new(8));
        let r = recordings.clone();
        let s = streams.clone();
        let connections = Arc::new(Semaphore::new(32));
        let make = hyper::service::make_service_fn(move |_| {
            let r = r.clone();
            let s = s.clone();
            let connection = connections.clone().try_acquire_owned();
            async move {
                let connection = connection
                    .map_err(|_| io::Error::other("Playback connection limit reached"))?;
                Ok::<_, io::Error>(hyper::service::service_fn(move |request| {
                    let _keep_alive = &connection;
                    serve(request, address, r.clone(), s.clone())
                }))
            }
        });
        let server = Server::from_tcp(listener)
            .map_err(|e| e.to_string())?
            .http1_only(true)
            .http1_max_buf_size(8192)
            .http1_header_read_timeout(Duration::from_secs(10))
            .serve(make);
        tokio::spawn(async move {
            if let Err(e) = server.await {
                log::warn!("Recording playback service stopped: {e}");
            }
        });
        Ok(Self {
            address,
            recordings,
            #[cfg(test)]
            streams,
        })
    }
}
pub(crate) async fn source(
    file: File,
    extension: &str,
    meeting_id: &str,
) -> Result<String, String> {
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("The recording must be a regular file".into());
    }
    let length = metadata.len();
    if length == 0 {
        return Err("This recording is empty".into());
    }
    let service = SERVICE.get_or_try_init(MediaService::start).await?;
    let content_type = match extension.to_ascii_lowercase().as_str() {
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "ogg" | "opus" => "audio/ogg",
        "m4a" | "mp4" => "audio/mp4",
        _ => "audio/wav",
    };
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let mut recordings = service
        .recordings
        .lock()
        .map_err(|_| "Recording playback lock unavailable")?;
    recordings.retain(|_, r| {
        r.lock()
            .map(|mut r| {
                r.revoked |= r.touched.elapsed() >= TTL;
                !r.revoked
            })
            .unwrap_or(false)
    });
    if recordings.len() >= MAX_RECORDINGS {
        let oldest = recordings
            .iter()
            .filter_map(|(k, r)| r.lock().ok().map(|r| (k.clone(), r.touched)))
            .min_by_key(|(_, t)| *t)
            .map(|(k, _)| k);
        if let Some(key) = oldest {
            if let Some(recording) = recordings.remove(&key) {
                if let Ok(mut recording) = recording.lock() {
                    recording.revoked = true;
                }
            }
        }
    }
    recordings.insert(
        token.clone(),
        Arc::new(Mutex::new(Recording {
            file,
            length,
            content_type,
            touched: Instant::now(),
            revoked: false,
            meeting_id: meeting_id.to_owned(),
        })),
    );
    Ok(format!("http://{}/{token}/recording", service.address))
}
pub(crate) fn revoke_meeting(meeting_id: &str) {
    let Some(service) = SERVICE.get() else { return };
    if let Ok(mut recordings) = service.recordings.lock() {
        recordings.retain(|_, recording| {
            if let Ok(mut r) = recording.lock() {
                if r.meeting_id == meeting_id {
                    r.revoked = true;
                    return false;
                }
            }
            true
        });
    }
}
pub(crate) fn release(source: &str) {
    let Some(service) = SERVICE.get() else { return };
    let prefix = format!("http://{}/", service.address);
    if let Some(token) = source
        .strip_prefix(&prefix)
        .and_then(|p| p.strip_suffix("/recording"))
    {
        if let Ok(mut r) = service.recordings.lock() {
            if let Some(recording) = r.remove(token) {
                if let Ok(mut recording) = recording.lock() {
                    recording.revoked = true;
                }
            }
        }
    }
}
fn empty(status: StatusCode) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("Cache-Control", "no-store")
        .body(Body::empty())
        .unwrap()
}
fn range(value: Option<&str>, length: u64) -> Result<(u64, u64, bool), ()> {
    if length == 0 {
        return Err(());
    }
    let Some(value) = value else {
        return Ok((0, length - 1, false));
    };
    let value = value.strip_prefix("bytes=").ok_or(())?;
    if value.contains(',') {
        return Err(());
    }
    let (start, end) = value.split_once('-').ok_or(())?;
    if start.is_empty() {
        let suffix: u64 = end.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        return Ok((length.saturating_sub(suffix), length - 1, true));
    }
    let start: u64 = start.parse().map_err(|_| ())?;
    let end: u64 = if end.is_empty() {
        length - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(length - 1)
    };
    if start >= length || end < start {
        return Err(());
    }
    Ok((start, end, true))
}
fn allowed_origin(origin: &str) -> bool {
    matches!(
        origin,
        "tauri://localhost"
            | "http://tauri.localhost"
            | "https://tauri.localhost"
            | "http://localhost:3118"
    )
}
async fn serve(
    request: Request<Body>,
    address: SocketAddr,
    recordings: Arc<Mutex<HashMap<String, Arc<Mutex<Recording>>>>>,
    streams: Arc<Semaphore>,
) -> Result<Response<Body>, Infallible> {
    if request.headers().get("host").and_then(|v| v.to_str().ok())
        != Some(address.to_string().as_str())
    {
        return Ok(empty(StatusCode::FORBIDDEN));
    }
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return Ok(empty(StatusCode::METHOD_NOT_ALLOWED));
    }
    let origin = request
        .headers()
        .get("origin")
        .and_then(|v| v.to_str().ok());
    if origin.is_some_and(|o| !allowed_origin(o)) {
        return Ok(empty(StatusCode::FORBIDDEN));
    }
    let key = request
        .uri()
        .path()
        .strip_prefix('/')
        .and_then(|p| p.strip_suffix("/recording"))
        .filter(|k| k.len() == 64 && k.bytes().all(|b| b.is_ascii_hexdigit()));
    let recording = key.and_then(|k| recordings.lock().ok()?.get(k).cloned());
    let Some(recording) = recording else {
        return Ok(empty(StatusCode::NOT_FOUND));
    };
    let (length, content_type) = match recording.lock() {
        Ok(mut r) if !r.revoked && r.touched.elapsed() < TTL => {
            r.touched = Instant::now();
            (r.length, r.content_type)
        }
        _ => return Ok(empty(StatusCode::NOT_FOUND)),
    };
    let range_header = match request.headers().get("range") {
        Some(v) => match v.to_str() {
            Ok(v) => Some(v),
            Err(_) => return Ok(empty(StatusCode::RANGE_NOT_SATISFIABLE)),
        },
        None => None,
    };
    let (start, end, partial) = match range(range_header, length) {
        Ok(r) => r,
        Err(()) => {
            return Ok(Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header("Content-Range", format!("bytes */{length}"))
                .body(Body::empty())
                .unwrap())
        }
    };
    let mut builder = Response::builder()
        .status(if partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header("Content-Type", content_type)
        .header("Content-Length", end - start + 1)
        .header("Accept-Ranges", "bytes")
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer");
    if partial {
        builder = builder.header("Content-Range", format!("bytes {start}-{end}/{length}"));
    }
    if let Some(o) = origin {
        builder = builder
            .header("Access-Control-Allow-Origin", o)
            .header("Vary", "Origin");
    }
    if request.method() == Method::HEAD {
        return Ok(builder.body(Body::empty()).unwrap());
    }
    let Ok(permit) = streams.try_acquire_owned() else {
        return Ok(empty(StatusCode::SERVICE_UNAVAILABLE));
    };
    let (mut sender, body) = Body::channel();
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut offset = start;
        while offset <= end {
            let mut data = vec![0_u8; (end - offset + 1).min(64 * 1024) as usize];
            let read = recording
                .lock()
                .map_err(|_| io::Error::other("Recording lock unavailable"))
                .and_then(|r| {
                    if r.revoked || r.touched.elapsed() >= TTL {
                        return Err(io::Error::other("Recording access revoked"));
                    }
                    use std::os::unix::fs::FileExt;
                    r.file.read_at(&mut data, offset)
                });
            match read {
                Ok(0) | Err(_) => {
                    sender.abort();
                    break;
                }
                Ok(n) => {
                    data.truncate(n);
                    offset += n as u64;
                    if !matches!(
                        runtime.block_on(async {
                            tokio::time::timeout(
                                Duration::from_secs(15),
                                sender.send_data(data.into()),
                            )
                            .await
                        }),
                        Ok(Ok(()))
                    ) {
                        break;
                    }
                }
            }
        }
    });
    Ok(builder.body(body).unwrap())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_requests_are_bounded_and_seekable() {
        assert_eq!(range(None, 100), Ok((0, 99, false)));
        assert_eq!(range(Some("bytes=20-40"), 100), Ok((20, 40, true)));
        assert_eq!(range(Some("bytes=20-"), 100), Ok((20, 99, true)));
        assert_eq!(range(Some("bytes=-10"), 100), Ok((90, 99, true)));
        assert_eq!(range(Some("bytes=20-999"), 100), Ok((20, 99, true)));
        for value in [
            "bytes=100-",
            "bytes=50-20",
            "bytes=-0",
            "bytes=0-1,3-4",
            "items=0-1",
            "bytes=18446744073709551616-",
        ] {
            assert!(range(Some(value), 100).is_err());
        }
        assert!(range(None, 0).is_err());
    }
    #[tokio::test]
    async fn recording_capability_supports_ranges_revocation_and_origin_checks() {
        use hyper::body::to_bytes;
        use std::io::Write;
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"0123456789").unwrap();
        let url = source(file, "wav", "test-meeting").await.unwrap();
        let service = SERVICE.get().unwrap();
        let build = |uri: &str| {
            Request::builder()
                .uri(uri)
                .header("host", service.address.to_string())
        };
        let response = serve(
            build(&url)
                .header("range", "bytes=3-5")
                .body(Body::empty())
                .unwrap(),
            service.address,
            service.recordings.clone(),
            service.streams.clone(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()["content-range"], "bytes 3-5/10");
        assert_eq!(&to_bytes(response.into_body()).await.unwrap()[..], b"345");
        let denied = serve(
            build(&url)
                .header("origin", "https://untrusted.example")
                .body(Body::empty())
                .unwrap(),
            service.address,
            service.recordings.clone(),
            service.streams.clone(),
        )
        .await
        .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        release(&url);
        let revoked = serve(
            build(&url).body(Body::empty()).unwrap(),
            service.address,
            service.recordings.clone(),
            service.streams.clone(),
        )
        .await
        .unwrap();
        assert_eq!(revoked.status(), StatusCode::NOT_FOUND);
        let file = tempfile::tempfile().unwrap();
        file.set_len(10).unwrap();
        let second = source(file, "wav", "erase-me").await.unwrap();
        let token = second.split('/').nth(3).unwrap();
        let outstanding = service
            .recordings
            .lock()
            .unwrap()
            .get(token)
            .unwrap()
            .clone();
        revoke_meeting("erase-me");
        assert!(outstanding.lock().unwrap().revoked);
        assert!(!service.recordings.lock().unwrap().contains_key(token));

        // Evicting a capability must also stop a body already streaming from it.
        let file = tempfile::tempfile().unwrap();
        file.set_len(10).unwrap();
        let expired = source(file, "wav", "expired").await.unwrap();
        let key = expired.split('/').nth(3).unwrap();
        let outstanding = service.recordings.lock().unwrap()[key].clone();
        outstanding.lock().unwrap().touched = Instant::now() - TTL - Duration::from_secs(1);
        let next = tempfile::tempfile().unwrap();
        next.set_len(10).unwrap();
        let next = source(next, "wav", "next").await.unwrap();
        assert!(
            outstanding.lock().unwrap().revoked,
            "Expiry left an active stream authorized"
        );
        release(&next);

        let mut links = Vec::new();
        let mut oldest = None;
        for index in 0..=MAX_RECORDINGS {
            let file = tempfile::tempfile().unwrap();
            file.set_len(10).unwrap();
            let url = source(file, "wav", "capacity-test").await.unwrap();
            if index == 0 {
                let key = url.split('/').nth(3).unwrap();
                let handle = service.recordings.lock().unwrap()[key].clone();
                handle.lock().unwrap().touched = Instant::now() - Duration::from_secs(60);
                oldest = Some(handle);
            }
            links.push(url);
        }
        assert!(
            oldest.unwrap().lock().unwrap().revoked,
            "Eviction left an active stream authorized"
        );
        for url in links {
            release(&url);
        }
    }

    #[tokio::test]
    async fn recording_capability_rejects_non_regular_handles() {
        let directory = tempfile::tempdir().unwrap();
        let file = File::open(directory.path()).unwrap();
        assert!(source(file, "wav", "invalid").await.is_err());
    }
}
