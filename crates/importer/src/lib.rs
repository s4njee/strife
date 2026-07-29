//! Watched-folder ingestion support for Strife.

use anyhow::Result;
use strife_storage::{DiskGuard, StorageBackend};

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
