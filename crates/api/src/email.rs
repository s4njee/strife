use std::{convert::Infallible, time::Duration};

use axum::{
    Json, Router,
    extract::{Path, Query, RawQuery, State},
    http::HeaderMap,
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use strife_db::{EmailExtractionStatus, EmailSearchCursor, EmailSearchFilters};
use uuid::Uuid;

use crate::error::ApiError;

const DEFAULT_LIMIT: u32 = 25;
const MAX_LIMIT: u32 = 100;
const FACET_LIMIT: i64 = 50;
const ROUTE: &str = "/api/email";

fn bad_request() -> ApiError {
    ApiError::BadRequest("The email request is invalid".into())
}

/// Repeatable filters mean the query string cannot go through a plain
/// `Deserialize`: `serde_urlencoded` keeps only the last value for a repeated
/// key, which would silently drop every `label` but one. The raw string is
/// parsed into ordered pairs instead.
struct SearchQuery {
    q: Option<String>,
    from: Vec<String>,
    participant: Vec<String>,
    label: Vec<String>,
    after: Option<DateTime<Utc>>,
    before: Option<DateTime<Utc>>,
    has_attachment: Option<bool>,
    status: Option<String>,
    thread_id: Option<Uuid>,
    duplicate_group: Option<Uuid>,
    include_trashed: bool,
    include_duplicates: bool,
    cursor: Option<String>,
    limit: Option<u32>,
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 3 <= bytes.len() => {
                if let Ok(byte) = u8::from_str_radix(&raw[index + 1..index + 3], 16) {
                    out.push(byte);
                    index += 3;
                } else {
                    out.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl SearchQuery {
    /// Parses the raw query string, rejecting unknown fields and excessive
    /// repetition rather than ignoring them.
    fn parse(raw: Option<&str>) -> Result<Self, ApiError> {
        const MAX_REPEATS: usize = 32;
        let mut query = Self {
            q: None,
            from: Vec::new(),
            participant: Vec::new(),
            label: Vec::new(),
            after: None,
            before: None,
            has_attachment: None,
            status: None,
            thread_id: None,
            duplicate_group: None,
            include_trashed: false,
            include_duplicates: false,
            cursor: None,
            limit: None,
        };
        let Some(raw) = raw.filter(|value| !value.is_empty()) else {
            return Ok(query);
        };
        for pair in raw.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = percent_decode(key);
            let value = percent_decode(value);
            let boolean = |value: &str| matches!(value, "true" | "1" | "yes" | "");
            match key.as_str() {
                "q" => query.q = Some(value),
                "from" => query.from.push(value),
                "participant" => query.participant.push(value),
                "label" => query.label.push(value),
                "after" => {
                    query.after = Some(value.parse().map_err(|_| bad_request())?);
                }
                "before" => {
                    query.before = Some(value.parse().map_err(|_| bad_request())?);
                }
                "has_attachment" => query.has_attachment = Some(boolean(&value)),
                "status" => query.status = Some(value),
                "thread_id" => {
                    query.thread_id = Some(value.parse().map_err(|_| bad_request())?);
                }
                "duplicate_group" => {
                    query.duplicate_group = Some(value.parse().map_err(|_| bad_request())?);
                }
                "include_trashed" => query.include_trashed = boolean(&value),
                "include_duplicates" => query.include_duplicates = boolean(&value),
                "cursor" => query.cursor = Some(value),
                "limit" => {
                    query.limit = Some(value.parse().map_err(|_| bad_request())?);
                }
                _ => return Err(bad_request()),
            }
        }
        if query.from.len() > MAX_REPEATS
            || query.participant.len() > MAX_REPEATS
            || query.label.len() > MAX_REPEATS
        {
            return Err(bad_request());
        }
        Ok(query)
    }
}

#[derive(Serialize)]
struct SearchHit {
    node_id: Uuid,
    subject: Option<String>,
    sent_at: Option<DateTime<Utc>>,
    snippet: String,
    attachment_count: i32,
    duplicate_count: i64,
    thread_count: i64,
    score: f32,
    from_address: Option<String>,
    from_display_name: Option<String>,
    labels: Vec<String>,
    /// Which parts of the message matched, so a result can explain itself.
    match_sources: Vec<String>,
    matched_attachment: Option<String>,
    matched_attachment_page: Option<i32>,
}

#[derive(Serialize)]
struct SearchResponse {
    results: Vec<SearchHit>,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
struct FacetBucket {
    value: String,
    count: i64,
}

#[derive(Serialize)]
struct FacetsResponse {
    labels: Vec<FacetBucket>,
    correspondents: Vec<FacetBucket>,
    years: Vec<FacetBucket>,
}

#[derive(Deserialize)]
struct MessageQuery {
    #[serde(default)]
    include_raw_headers: bool,
    /// Opt-in per request. Remote image URLs are withheld by default so a
    /// tracking pixel cannot fire merely because a message was opened.
    #[serde(default)]
    allow_remote_images: bool,
}

#[derive(Serialize)]
struct AddressResponse {
    role: String,
    display_name: Option<String>,
    address: String,
}

#[derive(Serialize)]
struct HeaderResponse {
    name: String,
    value: String,
}

#[derive(Serialize)]
struct AttachmentResponse {
    part_path: String,
    filename: Option<String>,
    media_type: String,
    disposition: Option<String>,
    content_id: Option<String>,
    decoded_size: Option<i64>,
    is_inline: bool,
    is_message: bool,
    extraction_status: String,
}

#[derive(Serialize)]
struct MessageResponse {
    node_id: Uuid,
    status: String,
    parser_version: String,
    message_id: Option<String>,
    in_reply_to: Option<String>,
    references: Vec<String>,
    subject: Option<String>,
    sent_at: Option<DateTime<Utc>>,
    received_at: Option<DateTime<Utc>>,
    body_text: String,
    /// Sanitized server-side. The reader never receives the original markup.
    body_html: Option<String>,
    /// How many remote subresources were stripped from `body_html`.
    blocked_remote_count: usize,
    /// Distinct hosts the message tried to contact, for the reveal warning.
    blocked_hosts: Vec<String>,
    preview_text: String,
    thread_group_id: Option<Uuid>,
    /// Why the message landed in that thread, so a weak grouping (a shared
    /// subject) can be told apart from a strong one (a References root).
    thread_reason: String,
    /// A provider thread id was used but the RFC headers disagree with it.
    thread_conflict: bool,
    duplicate_group_id: Option<Uuid>,
    duplicate_reason: String,
    provider_thread_id: Option<String>,
    labels: Vec<String>,
    addresses: Vec<AddressResponse>,
    attachments: Vec<AttachmentResponse>,
    warnings: Vec<String>,
    /// Present only when explicitly requested; the default response carries
    /// normalized fields only.
    raw_headers: Option<Vec<HeaderResponse>>,
}

/// Aggregate counts backing the sidebar badge and the Email page header.
///
/// Story 22.1 extends this with per-campaign state, throughput, and estimated
/// completion; the counts below are the subset the navigation surface needs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EmailCountsResponse {
    pub foreground_pending: i64,
    pub foreground_running: i64,
    pub backfill_pending: i64,
    pub backfill_running: i64,
    pub completed: i64,
    pub failed: i64,
    pub skipped: i64,
    pub unsupported: i64,
    pub remaining: i64,
    pub indexed: i64,
    pub candidates: i64,
    pub attachments_pending: i64,
    pub attachments_completed: i64,
    pub attachments_failed: i64,
}

/// One historical campaign's live progress.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EmailCampaignResponse {
    pub id: Uuid,
    pub state: String,
    pub snapshot_before: Option<DateTime<Utc>>,
    /// Durable resume position. A restart continues from here rather than
    /// restarting the scan.
    pub cursor_created_at: Option<DateTime<Utc>>,
    pub cursor_node_id: Option<Uuid>,
    pub batch_size: i32,
    pub max_queued: i32,
    pub max_running: i32,
    pub resource_class: String,
    pub foreground_fairness: i32,
    pub candidate_count: i64,
    pub enqueued_count: i64,
    pub completed_count: i64,
    pub failed_count: i64,
    pub skipped_count: i64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EmailStatusResponse {
    pub counts: EmailCountsResponse,
    /// Messages parsed per hour, measured from durable outcomes.
    pub completed_per_hour: i64,
    /// Seconds to finish the remaining queue at the observed rate. Absent when
    /// nothing has completed recently, because dividing by a rate of zero would
    /// produce a confident-looking infinity.
    pub eta_seconds: Option<i64>,
    pub parser_version: String,
    pub attachment_extractor_version: String,
    /// Each historical campaign separately, so a paused backfill is never
    /// confused with foreground mail that has stopped moving.
    pub campaigns: Vec<EmailCampaignResponse>,
}

#[derive(Clone, Debug, Serialize)]
struct EmailEventResponse {
    id: i64,
    node_id: Option<Uuid>,
    name: String,
    state: String,
    /// Present only for parsed messages, bounded by the writer. No body text,
    /// address, or raw header ever appears on this stream.
    subject: Option<String>,
    attachment_count: Option<i32>,
    duration_ms: Option<i64>,
    origin: String,
    campaign_id: Option<Uuid>,
    warning: Option<String>,
    created_at: DateTime<Utc>,
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/email/status", get(status))
        .route("/api/email/events", get(events))
        .route("/api/email/reprocess", post(reprocess))
        .route("/api/email/search", get(search))
        .route("/api/email/facets", get(facets))
        .route("/api/email/messages/{node_id}", get(message))
        .with_state(pool)
}

async fn status(State(pool): State<PgPool>) -> Result<Json<EmailStatusResponse>, ApiError> {
    load_status(&pool)
        .await
        .map(Json)
        .map_err(|error| ApiError::internal(error, ROUTE, "status"))
}

async fn load_status(pool: &PgPool) -> Result<EmailStatusResponse, sqlx::Error> {
    let counts = strife_db::email_status_counts(pool).await?;
    let campaigns: Vec<EmailCampaignResponse> = strife_db::list_backfill_campaigns(pool)
        .await?
        .into_iter()
        .filter(|campaign| campaign.kind == strife_db::BackfillKind::Email)
        .map(|campaign| EmailCampaignResponse {
            id: campaign.id,
            state: format!("{:?}", campaign.state).to_lowercase(),
            snapshot_before: campaign.snapshot_before,
            cursor_created_at: campaign.cursor_created_at,
            cursor_node_id: campaign.cursor_node_id,
            batch_size: campaign.batch_size,
            max_queued: campaign.max_queued,
            max_running: campaign.max_running,
            resource_class: format!("{:?}", campaign.resource_class).to_lowercase(),
            foreground_fairness: campaign.foreground_fairness,
            candidate_count: campaign.candidate_count,
            enqueued_count: campaign.enqueued_count,
            completed_count: campaign.completed_count,
            failed_count: campaign.failed_count,
            skipped_count: campaign.skipped_count,
            last_error: campaign.last_error,
        })
        .collect();

    // A rate of zero yields no estimate rather than an infinity: "unknown" is
    // more honest than a number that looks computed but means nothing.
    let completed_per_hour = counts.completed_last_hour;
    let eta_seconds = (completed_per_hour > 0 && counts.remaining > 0)
        .then(|| counts.remaining.saturating_mul(3600) / completed_per_hour);

    Ok(EmailStatusResponse {
        counts: EmailCountsResponse {
            foreground_pending: counts.foreground_pending,
            foreground_running: counts.foreground_running,
            backfill_pending: counts.backfill_pending,
            backfill_running: counts.backfill_running,
            completed: counts.completed,
            failed: counts.failed,
            skipped: counts.skipped,
            unsupported: counts.unsupported,
            remaining: counts.remaining,
            indexed: counts.indexed,
            candidates: counts.candidates,
            attachments_pending: counts.attachments_pending,
            attachments_completed: counts.attachments_completed,
            attachments_failed: counts.attachments_failed,
        },
        completed_per_hour,
        eta_seconds,
        parser_version: strife_media::EMAIL_PARSER_VERSION.to_owned(),
        attachment_extractor_version: ATTACHMENT_EXTRACTOR_VERSION.to_owned(),
        campaigns,
    })
}

#[derive(Deserialize)]
struct ReprocessRequest {
    /// `node`, `failed`, `missing`, or `version`.
    scope: String,
    node_id: Option<Uuid>,
    parser_version: Option<String>,
    /// Bounded server-side regardless of what is asked for; a retry that walks
    /// the whole archive is not a retry.
    limit: Option<u32>,
}

#[derive(Serialize)]
struct ReprocessResponse {
    enqueued: u64,
}

/// Requeues extraction for a single message or a bounded batch.
///
/// Per-file retry and bulk retry are the same operation with a different scope,
/// so they cannot drift apart in what they consider eligible. Every scope
/// excludes nodes that already have active work, which is what makes pressing
/// retry twice harmless.
async fn reprocess(
    State(pool): State<PgPool>,
    Json(request): Json<ReprocessRequest>,
) -> Result<Json<ReprocessResponse>, ApiError> {
    let scope = match request.scope.as_str() {
        "node" => strife_db::EmailReprocessScope::Node(request.node_id.ok_or_else(bad_request)?),
        "failed" => strife_db::EmailReprocessScope::Failed,
        "missing" => strife_db::EmailReprocessScope::Missing,
        "version" => strife_db::EmailReprocessScope::VersionMismatch(
            request
                .parser_version
                .unwrap_or_else(|| strife_media::EMAIL_PARSER_VERSION.to_owned()),
        ),
        _ => return Err(bad_request()),
    };
    let enqueued =
        strife_db::enqueue_email_reprocessing(&pool, &scope, request.limit.unwrap_or(50))
            .await
            .map_err(|error| ApiError::internal(error, ROUTE, "reprocess"))?;
    Ok(Json(ReprocessResponse { enqueued }))
}

/// Mirrors the worker's extractor version without depending on the worker crate.
///
/// The API and worker are separate binaries; duplicating the constant is
/// preferable to the API taking a dependency on the worker for one string. The
/// repair endpoint in Story 22.4 compares stored values against this.
pub const ATTACHMENT_EXTRACTOR_VERSION: &str = "1";

/// Streams extraction activity and status changes.
///
/// Resumes from `Last-Event-Id` so a reconnect does not replay the whole
/// history, and falls back to the newest id — not to zero — when no cursor is
/// supplied, because a fresh console wants live activity rather than a
/// ten-year backlog.
async fn events(
    State(pool): State<PgPool>,
    headers: HeaderMap,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let cursor = match headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
    {
        Some(cursor) => cursor,
        None => sqlx::query_scalar!("SELECT COALESCE(max(id), 0) AS \"cursor!\" FROM email_events")
            .fetch_one(&pool)
            .await
            .unwrap_or_default(),
    };

    let stream = futures_util::stream::unfold(
        (
            pool,
            cursor,
            Vec::<Event>::new(),
            None::<EmailStatusResponse>,
        ),
        |(pool, mut cursor, mut pending, mut previous_status)| async move {
            loop {
                if let Some(event) = pending.pop() {
                    return Some((
                        Ok::<_, Infallible>(event),
                        (pool, cursor, pending, previous_status),
                    ));
                }
                let records = match strife_db::list_email_events_after(&pool, cursor, 100).await {
                    Ok(records) => records,
                    Err(error) => {
                        tracing::error!(%error, "email event stream query failed");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                let current_status = match load_status(&pool).await {
                    Ok(status) => status,
                    Err(error) => {
                        tracing::error!(%error, "email status stream query failed");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                let mut next = Vec::with_capacity(records.len() + 1);
                for record in records {
                    cursor = record.id;
                    let response = EmailEventResponse {
                        id: record.id,
                        node_id: record.node_id,
                        name: record.node_name,
                        state: record.state,
                        subject: record.subject,
                        attachment_count: record.attachment_count,
                        duration_ms: record.duration_ms,
                        origin: format!("{:?}", record.origin).to_lowercase(),
                        campaign_id: record.campaign_id,
                        warning: record.warning,
                        created_at: record.created_at,
                    };
                    next.push(
                        Event::default()
                            .event("entry")
                            .id(record.id.to_string())
                            .json_data(response)
                            .unwrap_or_else(|_| Event::default().event("stream-error")),
                    );
                }
                // Status is emitted only when it changes, so an idle archive
                // does not push a full status payload twice a second.
                if previous_status.as_ref() != Some(&current_status) {
                    next.push(
                        Event::default()
                            .event("status")
                            .json_data(&current_status)
                            .unwrap_or_else(|_| Event::default().event("stream-error")),
                    );
                    previous_status = Some(current_status);
                }
                if next.is_empty() {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                } else {
                    pending = next.into_iter().rev().collect();
                }
            }
        },
    );
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// Encodes a stable page position as `score:sent_at_millis:node_id`.
fn encode_cursor(hit: &SearchHit) -> String {
    format!(
        "{}:{}:{}",
        hit.score,
        hit.sent_at
            .map_or(String::new(), |value| value.timestamp_millis().to_string()),
        hit.node_id
    )
}

fn decode_cursor(raw: &str) -> Option<EmailSearchCursor> {
    let mut parts = raw.split(':');
    let score = parts.next()?.parse::<f32>().ok()?;
    let sent_at_raw = parts.next()?;
    let node_id = parts.next()?.parse::<Uuid>().ok()?;
    let sent_at = if sent_at_raw.is_empty() {
        None
    } else {
        DateTime::from_timestamp_millis(sent_at_raw.parse::<i64>().ok()?)
    };
    Some(EmailSearchCursor {
        score,
        sent_at,
        node_id,
    })
}

fn parse_status(raw: &str) -> Option<EmailExtractionStatus> {
    match raw {
        "pending" => Some(EmailExtractionStatus::Pending),
        "completed" => Some(EmailExtractionStatus::Completed),
        "failed" => Some(EmailExtractionStatus::Failed),
        "skipped" => Some(EmailExtractionStatus::Skipped),
        "unsupported" => Some(EmailExtractionStatus::Unsupported),
        _ => None,
    }
}

fn status_name(status: EmailExtractionStatus) -> &'static str {
    match status {
        EmailExtractionStatus::Pending => "pending",
        EmailExtractionStatus::Completed => "completed",
        EmailExtractionStatus::Failed => "failed",
        EmailExtractionStatus::Skipped => "skipped",
        EmailExtractionStatus::Unsupported => "unsupported",
    }
}

async fn search(
    State(pool): State<PgPool>,
    RawQuery(raw): RawQuery,
) -> Result<Json<SearchResponse>, ApiError> {
    let query = SearchQuery::parse(raw.as_deref())?;
    let text = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let status = match query.status.as_deref() {
        Some(raw) => Some(parse_status(raw).ok_or_else(bad_request)?),
        None => None,
    };
    if let (Some(after), Some(before)) = (query.after, query.before) {
        if after >= before {
            return Err(bad_request());
        }
    }
    let filters = EmailSearchFilters {
        from: query.from,
        participant: query.participant,
        labels: query.label,
        after: query.after,
        before: query.before,
        has_attachment: query.has_attachment,
        status,
        thread_group_id: query.thread_id,
        duplicate_group_id: query.duplicate_group,
        include_trashed: query.include_trashed,
        include_duplicates: query.include_duplicates,
    };
    // An entirely unconstrained request would page the whole archive, so a
    // blank query is allowed only alongside at least one structured filter.
    if text.is_none() && filters.is_empty() {
        return Err(bad_request());
    }
    let cursor = match query.cursor.as_deref() {
        Some(raw) => Some(decode_cursor(raw).ok_or_else(bad_request)?),
        None => None,
    };
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let hits = strife_db::search_email(&pool, text, &filters, cursor, limit)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, "search"))?;
    let results: Vec<SearchHit> = hits
        .into_iter()
        .map(|hit| SearchHit {
            node_id: hit.node_id,
            subject: hit.subject,
            sent_at: hit.sent_at,
            snippet: hit.snippet,
            attachment_count: hit.attachment_count,
            duplicate_count: hit.duplicate_count,
            thread_count: hit.thread_count,
            score: hit.score,
            from_address: hit.from_address,
            from_display_name: hit.from_display_name,
            labels: hit.labels,
            match_sources: hit.match_sources,
            matched_attachment: hit.matched_attachment,
            matched_attachment_page: hit.matched_attachment_page,
        })
        .collect();
    let next_cursor = (u32::try_from(results.len()).unwrap_or(u32::MAX) == limit)
        .then(|| results.last().map(encode_cursor))
        .flatten();
    Ok(Json(SearchResponse {
        results,
        next_cursor,
    }))
}

async fn facets(State(pool): State<PgPool>) -> Result<Json<FacetsResponse>, ApiError> {
    let labels = strife_db::email_label_facets(&pool, FACET_LIMIT)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, "facets"))?;
    let correspondents = strife_db::email_correspondent_facets(&pool, FACET_LIMIT)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, "facets"))?;
    let years = strife_db::email_year_facets(&pool)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, "facets"))?;
    let bucket = |facet: strife_db::EmailFacet| FacetBucket {
        value: facet.value,
        count: facet.count,
    };
    Ok(Json(FacetsResponse {
        labels: labels.into_iter().map(bucket).collect(),
        correspondents: correspondents.into_iter().map(bucket).collect(),
        years: years.into_iter().map(bucket).collect(),
    }))
}

async fn message(
    State(pool): State<PgPool>,
    Path(node_id): Path<Uuid>,
    Query(query): Query<MessageQuery>,
) -> Result<Json<MessageResponse>, ApiError> {
    let record = strife_db::get_email_message(&pool, node_id)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, node_id))?
        .ok_or(ApiError::NotFound("Email message was not found"))?;
    let addresses = strife_db::list_email_addresses(&pool, node_id)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, node_id))?;
    let labels = strife_db::list_email_labels(&pool, node_id)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, node_id))?;
    let attachments = strife_db::list_email_attachments(&pool, node_id)
        .await
        .map_err(|error| ApiError::internal(error, ROUTE, node_id))?;
    let sanitized = sanitize_body(
        record.body_html.as_deref(),
        node_id,
        &attachments,
        query.allow_remote_images,
    );

    let mut warnings = record.warnings;
    if let Some(sanitized) = sanitized.as_ref() {
        warnings.extend(sanitized.warnings.iter().cloned());
    }

    let raw_headers = if query.include_raw_headers {
        Some(
            strife_db::list_email_headers(&pool, node_id)
                .await
                .map_err(|error| ApiError::internal(error, ROUTE, node_id))?
                .into_iter()
                .map(|header| HeaderResponse {
                    name: header.name,
                    value: header.value,
                })
                .collect(),
        )
    } else {
        None
    };

    Ok(Json(MessageResponse {
        node_id: record.node_id,
        status: status_name(record.status).to_owned(),
        parser_version: record.parser_version,
        message_id: record.message_id,
        in_reply_to: record.in_reply_to,
        references: record.reference_ids,
        subject: record.subject,
        sent_at: record.sent_at,
        received_at: record.received_at,
        body_text: record.body_text,
        body_html: sanitized.as_ref().map(|value| value.html.clone()),
        blocked_remote_count: sanitized
            .as_ref()
            .map_or(0, |value| value.blocked_remote_count),
        blocked_hosts: sanitized
            .as_ref()
            .map_or_else(Vec::new, |value| value.blocked_hosts.clone()),
        preview_text: record.preview_text,
        thread_group_id: record.thread_group_id,
        thread_reason: thread_reason_name(record.thread_reason).to_owned(),
        thread_conflict: record.thread_conflict,
        duplicate_group_id: record.duplicate_group_id,
        duplicate_reason: duplicate_reason_name(record.duplicate_reason).to_owned(),
        provider_thread_id: record.provider_thread_id,
        labels,
        addresses: addresses
            .into_iter()
            .map(|address| AddressResponse {
                role: role_name(address.role).to_owned(),
                display_name: address.display_name,
                address: address.address,
            })
            .collect(),
        attachments: attachments
            .into_iter()
            .map(|attachment| AttachmentResponse {
                part_path: attachment.part_path,
                filename: attachment.filename,
                media_type: attachment.media_type,
                disposition: attachment.disposition,
                content_id: attachment.content_id,
                decoded_size: attachment.decoded_size,
                is_inline: attachment.is_inline,
                is_message: attachment.is_message,
                extraction_status: status_name(attachment.extraction_status).to_owned(),
            })
            .collect(),
        warnings,
        raw_headers,
    }))
}

/// Sanitizes a message body for display.
///
/// Done here rather than in the browser so the original markup never crosses the
/// network at all. Inline `cid:` references are resolved only against the parts
/// this message actually declares, so one message can never reference another's.
fn sanitize_body(
    body_html: Option<&str>,
    node_id: Uuid,
    attachments: &[strife_db::EmailAttachmentRecord],
    allow_remote_images: bool,
) -> Option<strife_media::SanitizedHtml> {
    let inline: Vec<strife_media::InlinePart<'_>> = attachments
        .iter()
        .filter(|attachment| attachment.is_inline)
        .filter_map(|attachment| {
            attachment
                .content_id
                .as_deref()
                .map(|content_id| strife_media::InlinePart {
                    content_id,
                    part_path: attachment.part_path.as_str(),
                })
        })
        .collect();
    let options = strife_media::SanitizeOptions {
        node_id,
        inline: &inline,
        allow_remote_images,
    };
    body_html.map(|html| strife_media::sanitize_email_html(html, &options))
}

const fn thread_reason_name(reason: strife_db::EmailThreadReason) -> &'static str {
    match reason {
        strife_db::EmailThreadReason::Provider => "provider",
        strife_db::EmailThreadReason::References => "references",
        strife_db::EmailThreadReason::MessageId => "message_id",
        strife_db::EmailThreadReason::Subject => "subject",
        strife_db::EmailThreadReason::None => "none",
    }
}

const fn duplicate_reason_name(reason: strife_db::EmailDuplicateReason) -> &'static str {
    match reason {
        strife_db::EmailDuplicateReason::MessageId => "message_id",
        strife_db::EmailDuplicateReason::ContentHash => "content_hash",
        strife_db::EmailDuplicateReason::None => "none",
    }
}

const fn role_name(role: strife_db::EmailAddressRole) -> &'static str {
    match role {
        strife_db::EmailAddressRole::From => "from",
        strife_db::EmailAddressRole::Sender => "sender",
        strife_db::EmailAddressRole::ReplyTo => "reply_to",
        strife_db::EmailAddressRole::To => "to",
        strife_db::EmailAddressRole::Cc => "cc",
        strife_db::EmailAddressRole::Bcc => "bcc",
    }
}

#[cfg(test)]
mod tests {
    use super::{SearchHit, decode_cursor, encode_cursor};
    use uuid::Uuid;

    #[test]
    fn cursors_round_trip_including_a_missing_date() {
        let node_id = Uuid::new_v4();
        for sent_at in [Some(chrono::Utc::now()), None] {
            let hit = SearchHit {
                node_id,
                subject: None,
                sent_at,
                snippet: String::new(),
                attachment_count: 0,
                duplicate_count: 1,
                thread_count: 1,
                score: 0.25,
                from_address: None,
                from_display_name: None,
                labels: Vec::new(),
                match_sources: Vec::new(),
                matched_attachment: None,
                matched_attachment_page: None,
            };
            let decoded = decode_cursor(&encode_cursor(&hit)).expect("round trip");
            assert_eq!(decoded.node_id, node_id);
            assert!((decoded.score - 0.25).abs() < f32::EPSILON);
            assert_eq!(
                decoded.sent_at.map(|value| value.timestamp_millis()),
                sent_at.map(|value| value.timestamp_millis())
            );
        }
    }

    #[test]
    fn a_malformed_cursor_is_rejected() {
        for raw in ["", "nonsense", "1.0:notanumber:x", "1.0::not-a-uuid"] {
            assert!(decode_cursor(raw).is_none(), "{raw} was accepted");
        }
    }
}
