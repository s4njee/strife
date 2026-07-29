use std::path::PathBuf;

use strife_storage::{LocalFsBackend, StorageBackend, StorageKey};
use tokio::{fs, io::AsyncReadExt};
use uuid::Uuid;

fn temporary_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "strife-storage-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().simple()
    ))
}

#[tokio::test]
async fn local_backend_round_trips_ranges_and_reports_capacity() {
    let root = temporary_root();
    let source_path = root.with_extension("source");
    fs::write(&source_path, b"0123456789")
        .await
        .expect("write source fixture");
    let backend = LocalFsBackend::new(&root).await.expect("create backend");
    let key = StorageKey::original(Uuid::new_v4());

    let source = fs::File::open(&source_path).await.expect("open fixture");
    backend
        .put_stream(key, Box::pin(source))
        .await
        .expect("store fixture");
    assert!(backend.exists(key).await.expect("check object"));

    let mut full = backend.get_stream(key).await.expect("get object");
    let mut full_bytes = Vec::new();
    full.read_to_end(&mut full_bytes)
        .await
        .expect("read object");
    assert_eq!(full_bytes, b"0123456789");

    let mut range = backend.get_range(key, 3, 4).await.expect("get range");
    let mut range_bytes = Vec::new();
    range
        .read_to_end(&mut range_bytes)
        .await
        .expect("read range");
    assert_eq!(range_bytes, b"3456");

    let usage = backend.disk_usage().await.expect("read disk usage");
    assert!(usage.total_bytes > 0);
    assert!(usage.available_bytes <= usage.total_bytes);
    assert_eq!(usage.used_bytes, usage.total_bytes - usage.available_bytes);

    backend.delete(key).await.expect("delete object");
    backend.delete(key).await.expect("repeat delete");
    assert!(!backend.exists(key).await.expect("check deleted object"));
    for namespace in ["staging", "originals", "artifacts"] {
        assert!(root.join(namespace).is_dir());
    }

    fs::remove_file(source_path).await.expect("remove source");
    fs::remove_dir_all(root).await.expect("remove backend");
}
