use super::super::{StorageManager, list_managed_backend};
use super::*;
use futures_util::TryStreamExt;

#[tokio::test]
async fn local_storage_round_trip_and_list() {
    let dir = std::env::temp_dir().join(format!("grass-storage-test-{}", Uuid::now_v7()));
    let manager = StorageManager::new_local(&dir);
    let key = format!("avatars/users/{}/avatar.webp", Uuid::now_v7());
    let stored = manager.write_bytes(&key, b"webp-bytes").await.unwrap();
    assert_eq!(stored.size_bytes, 10);
    let opened = manager.open_artifact(&key).await.unwrap().unwrap();
    let bytes = opened
        .stream
        .try_collect::<Vec<_>>()
        .await
        .unwrap()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(bytes, b"webp-bytes");
    assert_eq!(
        list_managed_backend(&manager.backend())
            .await
            .unwrap()
            .len(),
        1
    );
    manager.remove(&key).await.unwrap();
    tokio::fs::remove_dir_all(dir).await.unwrap();
}

#[tokio::test]
async fn local_rename_rejects_a_missing_source_object() {
    let dir = std::env::temp_dir().join(format!("grass-storage-rename-{}", Uuid::now_v7()));
    let storage = LocalStorage::new(&dir);

    let error = ObjectStorage::rename(
        &storage,
        ".pending/missing",
        "deployments/project/deployment/grass-output.zip",
    )
    .await
    .unwrap_err();

    assert!(matches!(error, StorageError::Backend(_)));
    let _ = tokio::fs::remove_dir_all(dir).await;
}
