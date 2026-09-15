use super::super::{ObjectStream, StoredObjectMeta};
use super::*;
use async_trait::async_trait;
use futures_util::{StreamExt, stream};

#[tokio::test]
async fn build_log_append_rejects_content_over_the_hard_limit() {
    let dir = std::env::temp_dir().join(format!("grass-storage-log-limit-{}", Uuid::now_v7()));
    let manager = StorageManager::new_local(&dir);
    let content = "x".repeat(16 * 1024 * 1024 + 1);

    let error = manager
        .append_build_log(Uuid::now_v7(), Uuid::now_v7(), &content)
        .await
        .unwrap_err();

    assert!(matches!(error, StorageError::LimitExceeded { .. }));
    let _ = tokio::fs::remove_dir_all(dir).await;
}

#[tokio::test]
async fn build_log_read_rejects_an_oversized_stored_object() {
    let dir = std::env::temp_dir().join(format!("grass-storage-log-read-limit-{}", Uuid::now_v7()));
    let manager = StorageManager::new_local(&dir);
    let project_id = Uuid::now_v7();
    let deployment_id = Uuid::now_v7();
    let key = LocalStorage::build_log_relative_path(project_id, deployment_id);
    manager
        .write_bytes(&key, &vec![b'x'; 16 * 1024 * 1024 + 1])
        .await
        .unwrap();

    let error = manager
        .read_build_log(project_id, deployment_id)
        .await
        .unwrap_err();

    assert!(matches!(error, StorageError::LimitExceeded { .. }));
    tokio::fs::remove_dir_all(dir).await.unwrap();
}

#[derive(Clone)]
struct BlockingLogStorage {
    blocked_key: String,
    blocked_started: Arc<tokio::sync::Notify>,
    release_blocked: Arc<tokio::sync::Notify>,
    other_started: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl ObjectStorage for BlockingLogStorage {
    async fn put_bytes(&self, _key: &str, _content: &[u8]) -> Result<(), StorageError> {
        Ok(())
    }

    async fn put_stream(&self, _key: &str, mut stream: ObjectStream) -> Result<(), StorageError> {
        while stream.next().await.transpose()?.is_some() {}
        Ok(())
    }

    async fn open(&self, key: &str) -> Result<Option<OpenedObject>, StorageError> {
        if key == self.blocked_key {
            self.blocked_started.notify_one();
            self.release_blocked.notified().await;
        } else {
            self.other_started.notify_one();
        }
        Ok(None)
    }

    async fn remove(&self, _key: &str) -> Result<(), StorageError> {
        Ok(())
    }

    async fn rename(&self, _from: &str, _to: &str) -> Result<(), StorageError> {
        Ok(())
    }

    async fn list(&self, _prefix: &str) -> Result<Vec<StoredObjectMeta>, StorageError> {
        Ok(Vec::new())
    }

    async fn probe(&self) -> Result<(), StorageError> {
        Ok(())
    }
}

#[tokio::test]
async fn build_log_appends_for_independent_deployments_do_not_share_a_lock() {
    let project_id = Uuid::now_v7();
    let blocked_deployment = Uuid::now_v7();
    let other_deployment = Uuid::now_v7();
    let backend = Arc::new(BlockingLogStorage {
        blocked_key: LocalStorage::build_log_relative_path(project_id, blocked_deployment),
        blocked_started: Arc::new(tokio::sync::Notify::new()),
        release_blocked: Arc::new(tokio::sync::Notify::new()),
        other_started: Arc::new(tokio::sync::Notify::new()),
    });
    let blocked_started = Arc::clone(&backend.blocked_started);
    let release_blocked = Arc::clone(&backend.release_blocked);
    let other_started = Arc::clone(&backend.other_started);
    let manager = StorageManager::from_runtime(StorageRuntime {
        config: StorageConfig::local("/tmp/grass-storage-test"),
        backend,
    });

    let blocked = tokio::spawn({
        let manager = manager.clone();
        async move {
            manager
                .append_build_log(project_id, blocked_deployment, "blocked")
                .await
        }
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        blocked_started.notified(),
    )
    .await
    .expect("blocked deployment did not reach storage");

    let other = tokio::spawn({
        let manager = manager.clone();
        async move {
            manager
                .append_build_log(project_id, other_deployment, "other")
                .await
        }
    });
    let independent = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        other_started.notified(),
    )
    .await
    .is_ok();

    release_blocked.notify_one();
    blocked.await.unwrap().unwrap();
    other.await.unwrap().unwrap();
    assert!(
        independent,
        "independent deployment remained behind another log lock"
    );
}

#[tokio::test]
async fn maintenance_rejects_writes_but_keeps_reads_available() {
    let dir = std::env::temp_dir().join(format!("grass-storage-maintenance-{}", Uuid::now_v7()));
    let manager = StorageManager::new_local(&dir);
    let key = format!("avatars/users/{}/avatar.webp", Uuid::now_v7());
    manager.write_bytes(&key, b"existing").await.unwrap();

    manager.mark_maintenance();

    assert!(manager.open_artifact(&key).await.unwrap().is_some());
    assert!(matches!(
        manager.write_bytes("avatars/new.webp", b"new").await,
        Err(StorageError::Maintenance)
    ));
    assert!(matches!(
        manager
            .append_build_log(Uuid::now_v7(), Uuid::now_v7(), "log")
            .await,
        Err(StorageError::Maintenance)
    ));
    assert!(matches!(
        manager.remove(&key).await,
        Err(StorageError::Maintenance)
    ));

    manager.leave_maintenance();
    manager.remove(&key).await.unwrap();
    tokio::fs::remove_dir_all(dir).await.unwrap();
}

#[tokio::test]
async fn streamed_artifact_is_hashed_and_limited() {
    let dir = std::env::temp_dir().join(format!("grass-storage-stream-{}", Uuid::now_v7()));
    let manager = StorageManager::new_local(&dir);
    let chunks = stream::iter([
        Ok::<_, std::io::Error>(Bytes::from_static(b"zip-")),
        Ok(Bytes::from_static(b"bytes")),
    ]);
    let pending = manager
        .write_artifact_stream(Uuid::now_v7(), Uuid::now_v7(), chunks, 9)
        .await
        .unwrap();
    assert_eq!(pending.size_bytes, 9);
    let stored = pending.finalize().await.unwrap();
    assert_eq!(stored.size_bytes, 9);
    tokio::fs::remove_dir_all(dir).await.unwrap();
}

#[tokio::test]
async fn dropped_pending_artifact_removes_temporary_object() {
    let dir = std::env::temp_dir().join(format!("grass-storage-drop-{}", Uuid::now_v7()));
    let manager = StorageManager::new_local(&dir);
    let pending = manager
        .write_artifact_stream(
            Uuid::now_v7(),
            Uuid::now_v7(),
            stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"pending"))]),
            64,
        )
        .await
        .unwrap();
    let temporary_path = dir.join(pending.temporary_key.as_deref().unwrap());
    assert!(tokio::fs::try_exists(&temporary_path).await.unwrap());

    drop(pending);

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while tokio::fs::try_exists(&temporary_path).await.unwrap() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pending object cleanup timed out");
    tokio::fs::remove_dir_all(dir).await.unwrap();
}
