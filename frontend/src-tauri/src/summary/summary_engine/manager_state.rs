//! One summary-model manager per application, shared by startup and commands.
use super::model_manager::ModelManager;
use std::{future::Future, sync::Arc};
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::OnceCell;

#[derive(Default)]
pub struct ModelManagerState(OnceCell<Arc<ModelManager>>);

impl ModelManagerState {
    pub(super) fn ready(&self) -> Result<Arc<ModelManager>, String> {
        self.0
            .get()
            .cloned()
            .ok_or_else(|| "Model manager not initialized".into())
    }

    pub(super) async fn get_or_initialize<R: Runtime>(
        &self,
        app: &AppHandle<R>,
    ) -> Result<Arc<ModelManager>, String> {
        self.initialize(|| async {
            let directory = app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("models")
                .join("summary");
            let manager =
                ModelManager::new_with_models_dir(Some(directory)).map_err(|e| e.to_string())?;
            // Directory creation and scanning are idempotent if startup is cancelled.
            manager.init().await.map_err(|e| e.to_string())?;
            Ok(manager)
        })
        .await
    }

    async fn initialize<F, Fut>(&self, create: F) -> Result<Arc<ModelManager>, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<ModelManager, String>>,
    {
        self.0
            .get_or_try_init(|| async { create().await.map(Arc::new) })
            .await
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn concurrent_commands_share_one_manager_and_keep_its_identity() {
        let directory = tempfile::tempdir().unwrap();
        let state = Arc::new(ModelManagerState::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut jobs = tokio::task::JoinSet::new();
        for _ in 0..16 {
            let state = state.clone();
            let calls = calls.clone();
            let path = directory.path().to_path_buf();
            jobs.spawn(async move {
                state
                    .initialize(|| async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        tokio::task::yield_now().await;
                        ModelManager::new_with_models_dir(Some(path)).map_err(|e| e.to_string())
                    })
                    .await
                    .unwrap()
            });
        }
        let first = jobs.join_next().await.unwrap().unwrap();
        while let Some(result) = jobs.join_next().await {
            assert!(Arc::ptr_eq(&first, &result.unwrap()));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(&first, &state.ready().unwrap()));
    }

    #[tokio::test]
    async fn failed_initialization_can_retry_without_publishing_partial_state() {
        let directory = tempfile::tempdir().unwrap();
        let state = ModelManagerState::default();
        assert!(state
            .initialize(|| async { Err("scan failed".into()) })
            .await
            .is_err());
        assert!(state.ready().is_err());
        let manager = state
            .initialize(|| async {
                ModelManager::new_with_models_dir(Some(directory.path().to_path_buf()))
                    .map_err(|e| e.to_string())
            })
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&manager, &state.ready().unwrap()));
    }

    #[tokio::test]
    async fn cancelled_initialization_does_not_block_the_next_command() {
        let directory = tempfile::tempdir().unwrap();
        let state = Arc::new(ModelManagerState::default());
        let initializing = state.clone();
        let (entered, started) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            initializing
                .initialize(|| async {
                    entered.send(()).unwrap();
                    std::future::pending().await
                })
                .await
        });
        started.await.unwrap();
        task.abort();
        assert!(task.await.is_err_and(|error| error.is_cancelled()));
        assert!(state.ready().is_err());
        let manager = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            state.initialize(|| async {
                ModelManager::new_with_models_dir(Some(directory.path().to_path_buf()))
                    .map_err(|e| e.to_string())
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(Arc::ptr_eq(&manager, &state.ready().unwrap()));
    }
}
