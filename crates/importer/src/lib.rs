//! Watched-folder ingestion support for Strife.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use strife_db::{ImportEntryRecord, UpsertImportEntry};
use strife_storage::{DiskGuard, StorageBackend};
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
}
