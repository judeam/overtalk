//! Serialize large local inference; never modify display routing or refresh.
use once_cell::sync::Lazy;
use tokio::sync::{Mutex, MutexGuard};
use tokio_util::sync::CancellationToken;
static GPU: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
pub async fn acquire(token: &CancellationToken) -> Result<MutexGuard<'static, ()>, String> {
    tokio::select! {guard=GPU.lock()=>Ok(guard),_=token.cancelled()=>Err("Inference cancelled while waiting for the local model".into())}
}
