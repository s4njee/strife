//! Watched-folder ingestion support for Strife.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use strife_db::{FinalizeImport, ImportEntryRecord, NodeRecord, UpsertImportEntry};
use strife_storage::{DiskGuard, StorageBackend, StorageKey};
use tracing::debug;
use uuid::Uuid;

/// Scanner behavior that is safe for the fixed v1 inbox by default.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScanOptions {
    pub include_hidden: bool,
}

/// Summary of a single explicitly requested filesystem scan.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScanReport {
    pub directories: Vec<PathBuf>,
    pub files_discovered: usize,
    pub hidden_entries_skipped: usize,
    pub special_entries_skipped: usize,
}

/// Outcome of replaying entries interrupted at the importing checkpoint.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecoveryReport {
    pub attempted: usize,
    pub completed: usize,
    pub failures: Vec<(Uuid, String)>,
}

/// One regular-file observation made by the scanner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredFile {
    pub source_id: Uuid,
    pub relative_path: PathBuf,
    pub byte_size: u64,
    pub modified_at: DateTime<Utc>,
}

/// Persistence boundary used by the scanner.
#[async_trait]
pub trait DiscoverySink: Send + Sync {
    async fn upsert(&self, file: &DiscoveredFile) -> Result<()>;
}

/// PostgreSQL-backed discovery sink used by the production importer.
pub struct PostgresDiscoverySink<'a> {
    pool: &'a PgPool,
}

impl<'a> PostgresDiscoverySink<'a> {
    #[must_use]
    pub const fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DiscoverySink for PostgresDiscoverySink<'_> {
    async fn upsert(&self, file: &DiscoveredFile) -> Result<()> {
        let source_path = file
            .relative_path
            .to_str()
            .context("import path is not valid UTF-8")?;
        let source_size = i64::try_from(file.byte_size).context("import file is too large")?;
        let _: ImportEntryRecord = strife_db::upsert_import_entry(
            self.pool,
            UpsertImportEntry {
                source_id: file.source_id,
                source_path,
                source_size,
                source_modified_at: file.modified_at,
            },
        )
        .await?;
        Ok(())
    }
}

/// Recursively discovers regular files during one manual scan.
///
/// Directories are returned in parent-first order for hierarchy recreation.
/// Symlinks and all other special entries are deliberately not followed.
///
/// # Errors
///
/// Returns an error when the root cannot be read, file metadata is unavailable,
/// a relative path is invalid, or persistence fails.
pub async fn scan_directory(
    root: &Path,
    source_id: Uuid,
    options: ScanOptions,
    sink: &dyn DiscoverySink,
) -> Result<ScanReport> {
    let root_metadata = tokio::fs::symlink_metadata(root)
        .await
        .with_context(|| format!("inspect import root {}", root.display()))?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        anyhow::bail!("import root must be a directory and not a symbolic link");
    }

    let mut report = ScanReport::default();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut reader = tokio::fs::read_dir(&directory)
            .await
            .with_context(|| format!("read import directory {}", directory.display()))?;
        let mut paths = Vec::new();
        while let Some(entry) = reader.next_entry().await? {
            paths.push(entry.path());
        }
        paths.sort();
        paths.reverse();

        for path in paths {
            let relative_path = path
                .strip_prefix(root)
                .context("discovered path escaped import root")?
                .to_path_buf();
            if !options.include_hidden && is_hidden(&relative_path) {
                report.hidden_entries_skipped += 1;
                debug!(path = %path.display(), "skipping hidden import entry");
                continue;
            }
            let metadata = tokio::fs::symlink_metadata(&path)
                .await
                .with_context(|| format!("inspect import entry {}", path.display()))?;
            let file_type = metadata.file_type();
            if file_type.is_dir() {
                report.directories.push(relative_path);
                pending.push(path);
            } else if file_type.is_file() {
                let modified_at = metadata
                    .modified()
                    .with_context(|| format!("read modification time for {}", path.display()))?
                    .into();
                let modified_at = normalize_timestamp(modified_at);
                sink.upsert(&DiscoveredFile {
                    source_id,
                    relative_path,
                    byte_size: metadata.len(),
                    modified_at,
                })
                .await?;
                report.files_discovered += 1;
            } else {
                report.special_entries_skipped += 1;
                debug!(path = %path.display(), "skipping special import entry");
            }
        }
    }
    report.directories.sort();
    Ok(report)
}

/// Result of copying one source into staging while checking its stability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StabilityOutcome {
    Stable { staging_key: StorageKey },
    Changed,
}

/// Streams a discovered file into staging only when its observed metadata is
/// unchanged before and after the stream.
///
/// # Errors
///
/// Returns an error when the source cannot be inspected or opened, storage
/// streaming fails, or an unstable staging object cannot be cleaned up.
pub async fn stream_if_stable(
    storage: &dyn StorageBackend,
    source_path: &Path,
    expected_size: u64,
    expected_modified_at: DateTime<Utc>,
) -> Result<StabilityOutcome> {
    stream_if_stable_to(
        storage,
        source_path,
        expected_size,
        expected_modified_at,
        StorageKey::staging(Uuid::new_v4()),
    )
    .await
}

async fn stream_if_stable_to(
    storage: &dyn StorageBackend,
    source_path: &Path,
    expected_size: u64,
    expected_modified_at: DateTime<Utc>,
    staging_key: StorageKey,
) -> Result<StabilityOutcome> {
    let before = snapshot_regular_file(source_path).await?;
    if before != (expected_size, expected_modified_at) {
        return Ok(StabilityOutcome::Changed);
    }

    let file = tokio::fs::File::open(source_path)
        .await
        .with_context(|| format!("open import source {}", source_path.display()))?;
    storage.put_stream(staging_key, Box::pin(file)).await?;
    let after = snapshot_regular_file(source_path).await;
    if after.as_ref().ok() != Some(&before) {
        storage.delete(staging_key).await?;
        return Ok(StabilityOutcome::Changed);
    }
    Ok(StabilityOutcome::Stable { staging_key })
}

/// Stages one discovered database entry and persists the stability transition.
///
/// # Errors
///
/// Returns an error for unsafe paths, invalid sizes, filesystem/storage
/// failures, or a database transition failure.
pub async fn stage_import_entry(
    pool: &PgPool,
    storage: &dyn StorageBackend,
    watch_root: &Path,
    entry: &ImportEntryRecord,
) -> Result<StabilityOutcome> {
    let relative = Path::new(&entry.source_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        anyhow::bail!("import source path must remain below the watch root");
    }
    let expected_size = u64::try_from(entry.source_size).context("negative import file size")?;
    let outcome = stream_if_stable_to(
        storage,
        &watch_root.join(relative),
        expected_size,
        entry.source_modified_at,
        StorageKey::staging(entry.id),
    )
    .await?;
    match outcome {
        StabilityOutcome::Stable { .. } => {
            strife_db::mark_import_stable(pool, entry.id).await?;
        }
        StabilityOutcome::Changed => {
            strife_db::reset_import_discovered(pool, entry.id).await?;
        }
    }
    Ok(outcome)
}

/// Imports one discovered entry through capacity validation, stable staging,
/// content inspection, transactional publication, and source cleanup.
///
/// # Errors
///
/// Returns an error when the capacity guard, source validation, storage,
/// database finalization, or post-commit source cleanup fails. Actionable
/// failures are persisted on the import entry.
pub async fn import_entry(
    pool: &PgPool,
    storage: &dyn StorageBackend,
    watch_root: &Path,
    destination_folder_id: Uuid,
    entry: &ImportEntryRecord,
    guard: DiskGuard,
) -> Result<Option<NodeRecord>> {
    let result = import_entry_inner(
        pool,
        storage,
        watch_root,
        destination_folder_id,
        entry,
        guard,
    )
    .await;
    if let Err(error) = &result {
        let _ = strife_db::mark_import_failed(pool, entry.id, &error.to_string()).await;
    }
    result
}

async fn import_entry_inner(
    pool: &PgPool,
    storage: &dyn StorageBackend,
    watch_root: &Path,
    destination_folder_id: Uuid,
    entry: &ImportEntryRecord,
    guard: DiskGuard,
) -> Result<Option<NodeRecord>> {
    if let Some(node_id) = entry.resulting_node_id {
        let node = strife_db::get_node_by_id(pool, node_id)
            .await?
            .context("previously finalized import node is missing")?;
        let source_path = watch_root.join(&entry.source_path);
        match tokio::fs::remove_file(&source_path).await {
            Ok(()) => prune_empty_source_directories(watch_root, source_path.parent()).await,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("remove finalized import source {}", source_path.display())
                });
            }
        }
        let checksum = entry
            .source_checksum
            .as_deref()
            .context("previously finalized import checksum is missing")?;
        strife_db::mark_imported(pool, entry.id, node.id, checksum).await?;
        return Ok(Some(node));
    }
    let byte_size = u64::try_from(entry.source_size).context("negative import file size")?;
    ensure_import_capacity(storage, byte_size, guard).await?;
    let StabilityOutcome::Stable { staging_key } =
        stage_import_entry(pool, storage, watch_root, entry).await?
    else {
        return Ok(None);
    };
    let checksum = checksum_object(storage, staging_key).await?;
    let mime_type = storage.detect_mime(staging_key).await?;
    strife_db::mark_importing(pool, entry.id).await?;
    let original_key = StorageKey::original(entry.id);
    storage.move_object(staging_key, original_key).await?;

    let relative = Path::new(&entry.source_path);
    let display_name = relative
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .context("import file name is not valid UTF-8")?;
    let directory_names = relative
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let source_path = watch_root.join(relative);
    let source_created_at = tokio::fs::symlink_metadata(&source_path)
        .await
        .ok()
        .and_then(|metadata| metadata.created().ok())
        .map(Into::into);
    let finalized = strife_db::finalize_import(
        pool,
        FinalizeImport {
            entry_id: entry.id,
            destination_folder_id,
            directory_names: &directory_names,
            display_name,
            original_storage_key: original_key.id(),
            byte_size: entry.source_size,
            mime_type: &mime_type,
            checksum_sha256: &checksum,
            source_created_at,
            source_modified_at: entry.source_modified_at,
        },
    )
    .await;
    let node = match finalized {
        Ok(node) => node,
        Err(error) => {
            storage.delete(original_key).await?;
            return Err(error.into());
        }
    };
    tokio::fs::remove_file(&source_path)
        .await
        .with_context(|| format!("remove imported source {}", source_path.display()))?;
    prune_empty_source_directories(watch_root, source_path.parent()).await;
    Ok(Some(node))
}

/// Replays all entries that were durably checkpointed as importing before a
/// service interruption. One failure does not prevent recovery of later files.
///
/// # Errors
///
/// Returns an error only when the interrupted-entry list cannot be loaded.
pub async fn recover_interrupted_imports(
    pool: &PgPool,
    storage: &dyn StorageBackend,
    watch_root: &Path,
    destination_folder_id: Uuid,
    guard: DiskGuard,
) -> Result<RecoveryReport> {
    let entries = strife_db::list_importing_entries(pool).await?;
    let mut report = RecoveryReport::default();
    for entry in entries {
        report.attempted += 1;
        match import_entry(
            pool,
            storage,
            watch_root,
            destination_folder_id,
            &entry,
            guard,
        )
        .await
        {
            Ok(Some(_)) => report.completed += 1,
            Ok(None) => {}
            Err(error) => report.failures.push((entry.id, error.to_string())),
        }
    }
    Ok(report)
}

async fn checksum_object(storage: &dyn StorageBackend, key: StorageKey) -> Result<String> {
    use tokio::io::AsyncReadExt;

    let mut reader = storage.get_stream(key).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

async fn prune_empty_source_directories(root: &Path, start: Option<&Path>) {
    let mut current = start.map(Path::to_path_buf);
    while let Some(directory) = current {
        if directory == root || !directory.starts_with(root) {
            break;
        }
        match tokio::fs::remove_dir(&directory).await {
            Ok(()) => current = directory.parent().map(Path::to_path_buf),
            Err(_) => break,
        }
    }
}

async fn snapshot_regular_file(path: &Path) -> Result<(u64, DateTime<Utc>)> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .with_context(|| format!("inspect import source {}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        anyhow::bail!("import source is no longer a regular file");
    }
    let modified = metadata
        .modified()
        .with_context(|| format!("read modification time for {}", path.display()))?
        .into();
    Ok((metadata.len(), normalize_timestamp(modified)))
}

fn normalize_timestamp(value: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(value.timestamp_micros()).unwrap_or(value)
}

fn is_hidden(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| name.starts_with('.'))
    })
}

/// Applies the shared capacity guard before a watched-folder file is imported.
///
/// # Errors
///
/// Returns an error when disk usage cannot be read or the import would meet or
/// exceed the configured capacity threshold.
pub async fn ensure_import_capacity(
    storage: &dyn StorageBackend,
    incoming_bytes: u64,
    guard: DiskGuard,
) -> Result<u64> {
    let usage = storage.disk_usage().await?;
    guard
        .check(usage, incoming_bytes)
        .map_err(|error| anyhow::anyhow!("disk_full: {}% used", error.usage_percent))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use strife_storage::{DiskUsage, LocalFsBackend, StorageReader};

    use super::*;

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<DiscoveredFile>>);

    #[async_trait]
    impl DiscoverySink for RecordingSink {
        async fn upsert(&self, file: &DiscoveredFile) -> Result<()> {
            self.0.lock().expect("lock sink").push(file.clone());
            Ok(())
        }
    }

    struct MutatingStorage {
        inner: LocalFsBackend,
        source: PathBuf,
    }

    #[async_trait]
    impl StorageBackend for MutatingStorage {
        async fn put_stream(&self, key: StorageKey, reader: StorageReader) -> Result<()> {
            self.inner.put_stream(key, reader).await?;
            tokio::fs::write(&self.source, b"changed during staging").await?;
            Ok(())
        }

        async fn write_range(
            &self,
            key: StorageKey,
            offset: u64,
            reader: StorageReader,
        ) -> Result<u64> {
            self.inner.write_range(key, offset, reader).await
        }

        async fn move_object(&self, source: StorageKey, destination: StorageKey) -> Result<()> {
            self.inner.move_object(source, destination).await
        }

        async fn detect_mime(&self, key: StorageKey) -> Result<String> {
            self.inner.detect_mime(key).await
        }

        async fn get_stream(&self, key: StorageKey) -> Result<StorageReader> {
            self.inner.get_stream(key).await
        }

        async fn get_range(
            &self,
            key: StorageKey,
            offset: u64,
            length: u64,
        ) -> Result<StorageReader> {
            self.inner.get_range(key, offset, length).await
        }

        async fn delete(&self, key: StorageKey) -> Result<()> {
            self.inner.delete(key).await
        }

        async fn exists(&self, key: StorageKey) -> Result<bool> {
            self.inner.exists(key).await
        }

        async fn disk_usage(&self) -> Result<DiskUsage> {
            self.inner.disk_usage().await
        }
    }

    fn temporary_root() -> PathBuf {
        std::env::temp_dir().join(format!("strife-import-scan-{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn recursively_discovers_regular_visible_files() {
        let root = temporary_root();
        tokio::fs::create_dir_all(root.join("photos/2024"))
            .await
            .expect("create fixture directories");
        tokio::fs::create_dir_all(root.join(".private"))
            .await
            .expect("create hidden directory");
        tokio::fs::write(root.join("note.txt"), b"hello")
            .await
            .expect("write root file");
        tokio::fs::write(root.join("photos/2024/image.jpg"), b"image")
            .await
            .expect("write nested file");
        tokio::fs::write(root.join(".private/secret.txt"), b"secret")
            .await
            .expect("write hidden file");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("note.txt"), root.join("linked.txt"))
            .expect("create symlink");

        let sink = RecordingSink::default();
        let source_id = Uuid::new_v4();
        let report = scan_directory(&root, source_id, ScanOptions::default(), &sink)
            .await
            .expect("scan fixture");
        {
            let files = sink.0.lock().expect("lock results");
            assert_eq!(report.files_discovered, 2);
            assert_eq!(report.hidden_entries_skipped, 1);
            #[cfg(unix)]
            assert_eq!(report.special_entries_skipped, 1);
            assert_eq!(
                report.directories,
                vec![PathBuf::from("photos"), PathBuf::from("photos/2024")]
            );
            assert!(
                files
                    .iter()
                    .any(|file| file.relative_path == Path::new("note.txt"))
            );
            assert!(
                files
                    .iter()
                    .any(|file| file.relative_path == Path::new("photos/2024/image.jpg"))
            );
            assert!(files.iter().all(|file| file.source_id == source_id));
        }

        tokio::fs::remove_dir_all(&root)
            .await
            .expect("remove fixture");
    }

    #[tokio::test]
    async fn stable_file_is_staged_and_changed_snapshot_is_rejected() {
        let source_root = temporary_root();
        let storage_root = temporary_root();
        tokio::fs::create_dir_all(&source_root)
            .await
            .expect("create source root");
        let source = source_root.join("ready.txt");
        tokio::fs::write(&source, b"ready")
            .await
            .expect("write source");
        let snapshot = snapshot_regular_file(&source).await.expect("snapshot");
        let storage = LocalFsBackend::new(&storage_root)
            .await
            .expect("create storage");

        let stable = stream_if_stable(&storage, &source, snapshot.0, snapshot.1)
            .await
            .expect("stage stable file");
        let StabilityOutcome::Stable { staging_key } = stable else {
            panic!("unchanged file should be stable");
        };
        assert!(storage.exists(staging_key).await.expect("inspect staging"));

        tokio::fs::write(&source, b"ready")
            .await
            .expect("restore source");
        let snapshot = snapshot_regular_file(&source).await.expect("new snapshot");
        let mutating_storage = MutatingStorage {
            inner: storage.clone(),
            source: source.clone(),
        };
        assert_eq!(
            stream_if_stable(&mutating_storage, &source, snapshot.0, snapshot.1)
                .await
                .expect("reject mid-stream change"),
            StabilityOutcome::Changed
        );
        let mut staging_entries = tokio::fs::read_dir(storage_root.join("staging"))
            .await
            .expect("read staging directory");
        assert!(
            staging_entries
                .next_entry()
                .await
                .expect("inspect staging directory")
                .is_some(),
            "the first stable fixture remains staged"
        );
        assert!(
            staging_entries
                .next_entry()
                .await
                .expect("inspect staging cleanup")
                .is_none(),
            "changed file staging object was deleted"
        );

        tokio::fs::remove_dir_all(&source_root)
            .await
            .expect("remove source fixture");
        tokio::fs::remove_dir_all(&storage_root)
            .await
            .expect("remove storage fixture");
    }
}
