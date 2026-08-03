use axum::{
    Json, Router,
    extract::{Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::ApiError;

/// Builds internal metadata maintenance endpoints.
pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/admin/reprocess", post(reprocess))
        .route("/api/admin/email/versions", get(email_versions))
        .route("/api/admin/email/repair", get(email_repair))
        .route("/api/admin/email/repair/campaign", post(repair_campaign))
        .with_state(pool)
}

#[derive(Deserialize)]
struct ReprocessQuery {
    extractor: String,
    scope: Option<String>,
    node_id: Option<Uuid>,
}

#[derive(Serialize)]
struct ReprocessResponse {
    extractor: String,
    enqueued: u64,
}

async fn reprocess(
    State(pool): State<PgPool>,
    Query(query): Query<ReprocessQuery>,
) -> Result<Json<ReprocessResponse>, ApiError> {
    if query.extractor == "ocr" {
        let scope = match query.scope.as_deref() {
            Some("node") => strife_db::OcrReprocessScope::Node(query.node_id.ok_or_else(|| {
                ApiError::BadRequest("A node id is required for the node scope".into())
            })?),
            Some("failed") => strife_db::OcrReprocessScope::Failed,
            Some("version") => {
                let engine = strife_db::get_ocr_engine_state(&pool)
                    .await
                    .map_err(|error| ApiError::internal(error, "/api/admin", "admin"))?
                    .ok_or_else(|| {
                        ApiError::Unavailable("No OCR engine has reported a version yet".into())
                    })?;
                strife_db::OcrReprocessScope::VersionMismatch(engine.engine_version)
            }
            _ => return Err(ApiError::BadRequest("Unknown reprocess scope".into())),
        };
        let enqueued = strife_db::enqueue_ocr_reprocessing(&pool, &scope, 100)
            .await
            .map_err(|error| ApiError::internal(error, "/api/admin", "admin"))?;
        return Ok(Json(ReprocessResponse {
            extractor: query.extractor,
            enqueued,
        }));
    }
    if query.extractor == "email" {
        let scope = match query.scope.as_deref() {
            Some("node") => {
                strife_db::EmailReprocessScope::Node(query.node_id.ok_or_else(|| {
                    ApiError::BadRequest("A node id is required for the node scope".into())
                })?)
            }
            Some("failed") => strife_db::EmailReprocessScope::Failed,
            // Files finalized before the feature deployed, or whose enqueue
            // failed after finalization, have no projection at all.
            Some("missing") => strife_db::EmailReprocessScope::Missing,
            Some("version") => strife_db::EmailReprocessScope::VersionMismatch(
                strife_media::EMAIL_PARSER_VERSION.to_owned(),
            ),
            _ => return Err(ApiError::BadRequest("Unknown reprocess scope".into())),
        };
        let enqueued = strife_db::enqueue_email_reprocessing(&pool, &scope, 100)
            .await
            .map_err(|error| ApiError::internal(error, "/api/admin", "admin"))?;
        return Ok(Json(ReprocessResponse {
            extractor: query.extractor,
            enqueued,
        }));
    }
    let current_version = match query.extractor.as_str() {
        "exiftool" | "ffprobe" | "tika" => "adapter-v1",
        _ => return Err(ApiError::BadRequest("Unknown reprocess scope".into())),
    };
    let enqueued = strife_db::enqueue_reprocessing(&pool, &query.extractor, current_version)
        .await
        .map_err(|error| ApiError::internal(error, "/api/admin", "admin"))?;
    Ok(Json(ReprocessResponse {
        extractor: query.extractor,
        enqueued,
    }))
}

#[derive(Serialize)]
struct VersionCountResponse {
    version: String,
    count: i64,
}

#[derive(Serialize)]
struct EmailVersionsResponse {
    running_parser_version: String,
    running_attachment_extractor_version: String,
    parser: Vec<VersionCountResponse>,
    attachment_materializer: Vec<VersionCountResponse>,
    attachment_extractor: Vec<VersionCountResponse>,
    messages_needing_reparse: i64,
    attachments_needing_reextraction: i64,
    messages_needing_reindex: i64,
}

/// Reports which versions produced the stored data and how much is stale.
///
/// The axes are reported separately because a change to each implies different
/// work: reparsing a message is expensive, re-extracting one attachment is not,
/// and rebuilding a search vector touches no file at all.
async fn email_versions(
    State(pool): State<PgPool>,
) -> Result<Json<EmailVersionsResponse>, ApiError> {
    let parser_version = strife_media::EMAIL_PARSER_VERSION;
    let extractor_version = crate::email::ATTACHMENT_EXTRACTOR_VERSION;
    let report = strife_db::email_version_report(&pool, parser_version, extractor_version)
        .await
        .map_err(|error| ApiError::internal(error, "/api/admin", "admin"))?;
    let map = |counts: Vec<strife_db::VersionCount>| {
        counts
            .into_iter()
            .map(|entry| VersionCountResponse {
                version: entry.version,
                count: entry.count,
            })
            .collect()
    };
    Ok(Json(EmailVersionsResponse {
        running_parser_version: parser_version.to_owned(),
        running_attachment_extractor_version: extractor_version.to_owned(),
        parser: map(report.parser),
        attachment_materializer: map(report.attachment_materializer),
        attachment_extractor: map(report.attachment_extractor),
        messages_needing_reparse: report.messages_needing_reparse,
        attachments_needing_reextraction: report.attachments_needing_reextraction,
        messages_needing_reindex: report.messages_needing_reindex,
    }))
}

#[derive(Serialize)]
struct EmailRepairResponse {
    /// Always true: this endpoint has no write path at all, so "dry run" is a
    /// property of the code rather than a flag a caller could forget.
    dry_run: bool,
    missing_projections: i64,
    orphan_artifacts: i64,
    manifest_without_artifact: i64,
    artifact_without_manifest: i64,
    stale_leases: i64,
    campaigns_with_count_drift: i64,
    messages_needing_reindex: i64,
}

async fn email_repair(State(pool): State<PgPool>) -> Result<Json<EmailRepairResponse>, ApiError> {
    let report = strife_db::email_repair_scan(&pool)
        .await
        .map_err(|error| ApiError::internal(error, "/api/admin", "admin"))?;
    Ok(Json(EmailRepairResponse {
        dry_run: true,
        missing_projections: report.missing_projections,
        orphan_artifacts: report.orphan_artifacts,
        manifest_without_artifact: report.manifest_without_artifact,
        artifact_without_manifest: report.artifact_without_manifest,
        stale_leases: report.stale_leases,
        campaigns_with_count_drift: report.campaigns_with_count_drift,
        messages_needing_reindex: report.messages_needing_reindex,
    }))
}

#[derive(Deserialize)]
struct RepairCampaignRequest {
    campaign_id: Uuid,
}

#[derive(Serialize)]
struct RepairCampaignResponse {
    campaign_id: Uuid,
    /// Reported back so a caller can see the state was left alone.
    state: String,
    enqueued_count: i64,
}

/// Reconciles a campaign's counts. Cannot change its state.
async fn repair_campaign(
    State(pool): State<PgPool>,
    Json(request): Json<RepairCampaignRequest>,
) -> Result<Json<RepairCampaignResponse>, ApiError> {
    let campaign = strife_db::repair_campaign_counts(&pool, request.campaign_id)
        .await
        .map_err(|error| ApiError::internal(error, "/api/admin", "admin"))?;
    Ok(Json(RepairCampaignResponse {
        campaign_id: campaign.id,
        state: format!("{:?}", campaign.state).to_lowercase(),
        enqueued_count: campaign.enqueued_count,
    }))
}
