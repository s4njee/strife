//! Streams materialized email attachment artifacts.
//!
//! This is the endpoint a `cid:` reference in the message reader resolves to,
//! and the one an attachment download uses. Both are the same route because
//! both need the same guarantee: an attachment is addressed by the message that
//! carried it plus its MIME part path, and the lookup is scoped by that message.
//! A part belonging to a different message is not reachable through this path
//! whatever the caller supplies, and no storage key is ever accepted from the
//! request — it is read from the artifact row.
//!
//! Rendering safety follows the same allowlist as Strife's own file preview, so
//! the archive and the file browser cannot drift into two different answers
//! about what may be shown inline. Anything not on that list downloads.

use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use sqlx::PgPool;
use strife_db::{EmailArtifactState, JobType};
use strife_storage::{StorageBackend, StorageKey};
use tokio_util::io::ReaderStream;
use tracing::error;
use uuid::Uuid;

use crate::files::{
    is_native_preview_mime, parse_range, range_not_satisfiable, safe_filename, status_response,
};

const ROUTE: &str = "/api/email/messages/{node_id}/parts/{part_path}";
const INTERNAL: &str = "The attachment request could not be completed";

#[derive(Clone)]
struct PartState {
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
}

#[derive(Deserialize)]
struct PartQuery {
    /// Requests inline rendering. Honoured only for allowlisted types; anything
    /// else is served as a download regardless of what the caller asked for.
    #[serde(default)]
    inline: bool,
}

/// Builds the attachment streaming router.
pub fn router(pool: PgPool, storage: Arc<dyn StorageBackend>) -> Router {
    Router::new()
        .route(
            "/api/email/messages/{node_id}/parts/{part_path}",
            get(attachment_part),
        )
        .with_state(PartState { pool, storage })
}

#[allow(clippy::too_many_lines)]
async fn attachment_part(
    State(state): State<PartState>,
    Path((node_id, part_path)): Path<(Uuid, String)>,
    axum::extract::Query(query): axum::extract::Query<PartQuery>,
    headers: HeaderMap,
) -> Response {
    // The message must still be readable. A trashed or purged message must not
    // keep serving its attachments through a link someone kept.
    match strife_db::get_email_message(&state.pool, node_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return status_response(StatusCode::NOT_FOUND),
        Err(error) => {
            error!(%error, route = ROUTE, %node_id, "failed to load message for attachment");
            return status_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Scoped by node_id: this is the same-message authorization. There is no
    // code path that reaches an artifact of another message from here.
    let artifact = match strife_db::get_email_attachment_artifact(&state.pool, node_id, &part_path)
        .await
    {
        Ok(Some(artifact)) => artifact,
        Ok(None) => return status_response(StatusCode::NOT_FOUND),
        Err(error) => {
            error!(%error, route = ROUTE, %node_id, %part_path, "failed to load attachment artifact");
            return status_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    if artifact.state != EmailArtifactState::Ready {
        return rematerialize(&state, node_id, artifact.state).await;
    }
    let Some(storage_key) = artifact.storage_key.as_deref() else {
        return rematerialize(&state, node_id, artifact.state).await;
    };
    let object_id = match Uuid::parse_str(storage_key) {
        Ok(object_id) => object_id,
        Err(error) => {
            error!(%error, route = ROUTE, %node_id, %part_path, "attachment artifact has an unusable storage key");
            return status_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // The manifest supplies the display filename. It is sender-controlled, so it
    // is sanitized before it reaches a header and is never used as a path.
    let filename = match strife_db::list_email_attachments(&state.pool, node_id).await {
        Ok(attachments) => attachments
            .into_iter()
            .find(|attachment| attachment.part_path == part_path)
            .and_then(|attachment| attachment.filename),
        Err(error) => {
            error!(%error, route = ROUTE, %node_id, "failed to load attachment manifest");
            None
        }
    };
    let filename = filename.unwrap_or_else(|| format!("attachment-{part_path}"));

    let media_type = artifact.media_type.as_str();
    // An inline request is a request, not an instruction.
    let inline = query.inline && is_native_preview_mime(media_type);
    let total = match u64::try_from(artifact.byte_size) {
        Ok(total) => total,
        Err(error) => {
            error!(%error, route = ROUTE, %node_id, %part_path, "attachment artifact has a negative byte size");
            return status_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let requested_range = match headers.get(header::RANGE) {
        Some(value) => match parse_range(value, total) {
            Ok(range) => Some(range),
            Err(()) => return range_not_satisfiable(total),
        },
        None => None,
    };
    let (status, start, length) = requested_range.map_or((StatusCode::OK, 0, total), |range| {
        (StatusCode::PARTIAL_CONTENT, range.start, range.length())
    });

    let key = StorageKey::artifact(object_id);
    let reader = if requested_range.is_some() {
        state.storage.get_range(key, start, length).await
    } else {
        state.storage.get_stream(key).await
    };
    let Ok(reader) = reader else {
        // The row says ready but the bytes are gone. Rebuilding is possible
        // because the .eml original is canonical.
        return rematerialize(&state, node_id, artifact.state).await;
    };

    let mut response = Response::builder()
        .status(status)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, length.to_string())
        // Declared, never sniffed: a mislabelled attachment must not be
        // re-interpreted by the browser into something executable.
        .header(header::CONTENT_TYPE, media_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "{}; filename=\"{}\"",
                if inline { "inline" } else { "attachment" },
                safe_filename(&filename)
            ),
        )
        .header("x-content-type-options", "nosniff")
        // Defence in depth for the inline case: even an allowlisted type is
        // served under a policy that forbids script and subresource loads.
        .header(
            "content-security-policy",
            "default-src 'none'; img-src 'self' data:; media-src 'self'; \
             style-src 'unsafe-inline'; sandbox",
        );
    if let Some(range) = requested_range {
        response = response.header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", range.start, range.end, total),
        );
    }
    response
        .body(Body::from_stream(ReaderStream::new(reader)))
        .unwrap_or_else(|error| {
            crate::error::ApiError::internal_with(error, ROUTE, node_id, INTERNAL).into_response()
        })
}

/// Reports an artifact that is not servable and schedules a bounded rebuild.
///
/// The state is named rather than collapsed into a 404 so a caller can tell
/// "this attachment failed to materialize" from "no such attachment".
async fn rematerialize(state: &PartState, node_id: Uuid, current: EmailArtifactState) -> Response {
    let status = match current {
        EmailArtifactState::Ready | EmailArtifactState::Pending => "regenerating",
        EmailArtifactState::Failed => "failed",
        EmailArtifactState::Skipped => "skipped",
    };
    // Re-running the email job rewrites every artifact for this message from
    // the canonical original; enqueueing is best effort because an existing
    // pending job is the same outcome.
    if matches!(
        current,
        EmailArtifactState::Ready | EmailArtifactState::Pending | EmailArtifactState::Failed
    ) {
        if let Err(error) =
            strife_db::enqueue_job(&state.pool, JobType::EmailExtraction, node_id, 0).await
        {
            error!(%error, route = ROUTE, %node_id, "failed to enqueue attachment rematerialization");
        }
    }
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "status": status })),
    )
        .into_response()
}
