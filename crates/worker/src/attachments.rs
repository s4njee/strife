//! Turns decoded email attachment parts into managed, regenerable artifacts.
//!
//! The `.eml` original stays canonical; everything written here is disposable
//! and can be rebuilt by reparsing it. That is what makes a rerun safe to do at
//! any time and what lets a failed part be retried without touching the rest of
//! the message.
//!
//! Two properties are load-bearing:
//!
//! - **Storage keys come from identity, never from filenames.** The key is a
//!   `UUIDv5` of the message node and the MIME part path. A sender controls the
//!   filename and could otherwise aim two attachments at one object, or steer
//!   bytes somewhere they do not belong. Nothing here ever joins a filename onto
//!   a path.
//! - **One bad attachment does not fail the message.** A part that is too large,
//!   malformed, or unwritable is recorded as a failed artifact with a reason;
//!   the message's own extraction stays `completed`, because the message parsed
//!   fine and its text is searchable regardless.

use std::sync::Arc;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use strife_db::{
    EmailArtifactState, UpsertEmailAttachmentArtifact, email_attachment_artifact_id,
    upsert_email_attachment_artifact,
};
use strife_media::AttachmentPart;
use strife_storage::{StorageBackend, StorageKey};
use tracing::warn;
use uuid::Uuid;

/// Bumped when a change would make previously written artifacts wrong rather
/// than merely older, so a version mismatch can drive bounded rematerialization.
pub const MATERIALIZER_VERSION: &str = "1";

/// Bytes hashed and written per chunk.
///
/// Chunking keeps hashing and writing in one pass over the part instead of
/// buying a second full-size buffer to digest separately.
const CHUNK_BYTES: usize = 64 * 1024;

/// Outcome counts for one message's materialization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MaterializationSummary {
    pub written: usize,
    pub failed: usize,
    pub total_bytes: u64,
}

/// Writes every decoded part of one message, replacing any previous artifacts.
///
/// Never returns an error for a single unwritable part: each part's outcome is
/// recorded on its own row and the summary reports how many failed. An error
/// here means the whole batch could not be attempted.
pub async fn materialize(
    pool: &PgPool,
    storage: &Arc<dyn StorageBackend>,
    node_id: Uuid,
    parts: Vec<AttachmentPart>,
) -> Result<MaterializationSummary> {
    let mut summary = MaterializationSummary::default();
    // Parts are consumed rather than borrowed so each buffer can be moved into
    // the writer. Borrowing would force a full-size copy per part, which is the
    // opposite of what bounded materialization is for.
    for part in parts {
        match write_part(storage, node_id, &part.part_path, part.bytes).await {
            Ok(written) => {
                upsert_email_attachment_artifact(
                    pool,
                    &UpsertEmailAttachmentArtifact {
                        node_id,
                        part_path: &part.part_path,
                        state: EmailArtifactState::Ready,
                        storage_key: Some(&written.storage_key),
                        media_type: &part.media_type,
                        byte_size: i64::try_from(written.byte_size).unwrap_or(i64::MAX),
                        checksum_sha256: Some(&written.checksum),
                        depth: i32::try_from(part.depth).unwrap_or(i32::MAX),
                        is_message: part.is_message,
                        materializer_version: MATERIALIZER_VERSION,
                        warnings: &[],
                    },
                )
                .await
                .context("record attachment artifact")?;
                summary.written += 1;
                summary.total_bytes += written.byte_size;
            }
            Err(error) => {
                let detail = format!("{error:#}");
                warn!(
                    node_id = %node_id,
                    part_path = %part.part_path,
                    error = %detail,
                    "attachment materialization failed"
                );
                // The row is kept rather than dropped so the failure is
                // visible and can be targeted by a bounded reprocess later.
                upsert_email_attachment_artifact(
                    pool,
                    &UpsertEmailAttachmentArtifact {
                        node_id,
                        part_path: &part.part_path,
                        state: EmailArtifactState::Failed,
                        storage_key: None,
                        media_type: &part.media_type,
                        byte_size: 0,
                        checksum_sha256: None,
                        depth: i32::try_from(part.depth).unwrap_or(i32::MAX),
                        is_message: part.is_message,
                        materializer_version: MATERIALIZER_VERSION,
                        warnings: std::slice::from_ref(&detail),
                    },
                )
                .await
                .context("record failed attachment artifact")?;
                summary.failed += 1;
            }
        }
    }
    Ok(summary)
}

struct WrittenPart {
    storage_key: String,
    byte_size: u64,
    checksum: String,
}

/// Writes one part's bytes and returns what was actually stored.
///
/// A rerun overwrites the same object, because the key is derived from identity
/// rather than generated per attempt. If the write fails partway the object is
/// deleted, so a later read can never find a truncated artifact whose checksum
/// would not match its source.
async fn write_part(
    storage: &Arc<dyn StorageBackend>,
    node_id: Uuid,
    part_path: &str,
    bytes: Vec<u8>,
) -> Result<WrittenPart> {
    let id = email_attachment_artifact_id(node_id, part_path);
    let key = StorageKey::artifact(id);

    let mut digest = Sha256::new();
    for chunk in bytes.chunks(CHUNK_BYTES) {
        digest.update(chunk);
    }
    let checksum = format!("{:x}", digest.finalize());
    let byte_size = bytes.len() as u64;

    // A prior artifact is removed first: `put_stream` creates its destination
    // exclusively, so overwriting in place is not available to it.
    if storage.exists(key).await.unwrap_or(false) {
        storage.delete(key).await.context("remove prior artifact")?;
    }

    let reader = Box::pin(std::io::Cursor::new(bytes));
    match storage.put_stream(key, reader).await {
        Ok(()) => Ok(WrittenPart {
            storage_key: id.to_string(),
            byte_size,
            checksum,
        }),
        Err(error) => {
            // Partial output must not survive a failed write.
            let _ = storage.delete(key).await;
            Err(error.context("write attachment artifact"))
        }
    }
}
