//! The service's loopback HTTP/1 listeners (the local API and Linux recording
//! playback) share one accept loop. A connection lease is taken before headers
//! are parsed, so idle sockets and slow clients are bounded, not only requests.
use std::{convert::Infallible, future::Future, io, sync::Arc, time::Duration};

use hyper::{
    body::{Body, Incoming},
    server::conn::http1,
    service::service_fn,
    Request, Response,
};
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    server::graceful::GracefulShutdown,
};
use tokio::{net::TcpListener, sync::Semaphore};

pub(crate) struct Limits {
    /// Open connections, including ones still sending headers.
    pub connections: usize,
    /// Largest buffered request head.
    pub max_buf_size: usize,
}

/// Serve `handler` until `shutdown` resolves, then let open requests finish.
pub(crate) async fn serve<H, F, B>(
    listener: TcpListener,
    limits: Limits,
    shutdown: impl Future<Output = ()>,
    handler: H,
) where
    H: Fn(Request<Incoming>) -> F + Clone + Send + Sync + 'static,
    F: Future<Output = Response<B>> + Send + 'static,
    B: Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let leases = Arc::new(Semaphore::new(limits.connections));
    let mut http = http1::Builder::new();
    http.timer(TokioTimer::new())
        .max_buf_size(limits.max_buf_size)
        .header_read_timeout(Duration::from_secs(10));
    let graceful = GracefulShutdown::new();
    tokio::pin!(shutdown);
    loop {
        let stream = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(error) => {
                    // As hyper 0.14 did: retry at once after a failed handshake,
                    // back off when the process is out of descriptors or memory.
                    if !matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::ConnectionRefused
                            | io::ErrorKind::ConnectionReset
                    ) {
                        log::warn!("Loopback listener accept failed: {error}");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    continue;
                }
            },
            () = &mut shutdown => break,
        };
        // Over the limit, dropping the stream closes it before any parsing.
        let Ok(lease) = leases.clone().try_acquire_owned() else {
            continue;
        };
        let handler = handler.clone();
        let service = service_fn(move |request| {
            let response = handler(request);
            async move { Ok::<_, Infallible>(response.await) }
        });
        let connection = graceful.watch(http.serve_connection(TokioIo::new(stream), service));
        tokio::spawn(async move {
            let _lease = lease;
            let _ = connection.await;
        });
    }
    graceful.shutdown().await;
}
