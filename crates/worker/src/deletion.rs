//! Permanent deletion of trashed nodes and their managed storage objects.

use std::sync::Arc;

use anyhow::{Context, Result};
use sqlx::PgPool;
use strife_db::{
    JobRecord, list_storage_keys_for_deletion, purge_trashed_node_records,
};
use strife_storage::{StorageBackend, StorageKey};
use tracing::info;
use uuid::Uuid;

/// Deletes originals, derived artifacts, and database rows for a trashed tree.
pub struct DeletionService {
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
}

impl DeletionService {
    #[must_use]
    pub fn new(pool: PgPool, storage: Arc<dyn StorageBackend>) -> Self {
        Self { pool, storage }
    }

    /// Runs permanent deletion for the job's target node. Idempotent when the
    /// node is already gone.
    ///
    /// # Errors
    ///
    /// Returns an error when storage or database cleanup fails (so the job can retry).
    pub async fn purge(&self, job: &JobRecord) -> Result<()> {
        let keys = list_storage_keys_for_deletion(&self.pool, job.target_node_id)
            .await
            .context("list storage keys for permanent deletion")?;

        if keys.is_empty() {
            info!(node_id = %job.target_node_id, "permanent deletion target already gone");
            return Ok(());
        }

        for entry in &keys {
            if let Some(original) = &entry.original_storage_key {
                delete_original(self.storage.as_ref(), original).await?;
            }
            for artifact_key in &entry.artifact_storage_keys {
                delete_artifact(self.storage.as_ref(), artifact_key).await?;
            }
        }

        let removed = purge_trashed_node_records(&self.pool, job.target_node_id)
            .await
            .context("purge trashed node records")?;
        info!(
            node_id = %job.target_node_id,
            removed,
            "permanent deletion completed"
        );
        Ok(())
    }
}

async fn delete_original(storage: &dyn StorageBackend, key: &str) -> Result<()> {
    let id = parse_storage_id(key)?;
    storage
        .delete(StorageKey::original(id))
        .await
        .with_context(|| format!("delete original storage object {key}"))
}

async fn delete_artifact(storage: &dyn StorageBackend, key: &str) -> Result<()> {
    let id = parse_storage_id(key)?;
    storage
        .delete(StorageKey::artifact(id))
        .await
        .with_context(|| format!("delete artifact storage object {key}"))
}

fn parse_storage_id(key: &str) -> Result<Uuid> {
    Uuid::parse_str(key).with_context(|| format!("invalid storage key {key}"))
}
