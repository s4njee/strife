use axum::{
    Json, Router,
    extract::{Path, Query, RawQuery, State},
    http::StatusCode,
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use strife_db::{EmailExtractionStatus, EmailSearchCursor, EmailSearchFilters};
use uuid::Uuid;

use crate::internal_error;

const DEFAULT_LIMIT: u32 = 25;
const MAX_LIMIT: u32 = 100;
const FACET_LIMIT: i64 = 50;

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
    fn parse(raw: Option<&str>) -> Result<Self, StatusCode> {
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
                    query.after = Some(value.parse().map_err(|_| StatusCode::BAD_REQUEST)?);
                }
                "before" => {
                    query.before = Some(value.parse().map_err(|_| StatusCode::BAD_REQUEST)?);
                }
                "has_attachment" => query.has_attachment = Some(boolean(&value)),
                "status" => query.status = Some(value),
                "thread_id" => {
                    query.thread_id = Some(value.parse().map_err(|_| StatusCode::BAD_REQUEST)?);
                }
                "duplicate_group" => {
                    query.duplicate_group =
                        Some(value.parse().map_err(|_| StatusCode::BAD_REQUEST)?);
                }
                "include_trashed" => query.include_trashed = boolean(&value),
                "include_duplicates" => query.include_duplicates = boolean(&value),
                "cursor" => query.cursor = Some(value),
                "limit" => {
                    query.limit = Some(value.parse().map_err(|_| StatusCode::BAD_REQUEST)?);
                }
                _ => return Err(StatusCode::BAD_REQUEST),
            }
        }
        if query.from.len() > MAX_REPEATS
            || query.participant.len() > MAX_REPEATS
            || query.label.len() > MAX_REPEATS
        {
            return Err(StatusCode::BAD_REQUEST);
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
    body_html: Option<String>,
    preview_text: String,
    thread_group_id: Option<Uuid>,
    duplicate_group_id: Option<Uuid>,
    provider_thread_id: Option<String>,
    labels: Vec<String>,
    addresses: Vec<AddressResponse>,
    attachments: Vec<AttachmentResponse>,
    warnings: Vec<String>,
    /// Present only when explicitly requested; the default response carries
    /// normalized fields only.
    raw_headers: Option<Vec<HeaderResponse>>,
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/api/email/search", get(search))
        .route("/api/email/facets", get(facets))
        .route("/api/email/messages/{node_id}", get(message))
        .with_state(pool)
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
) -> Result<Json<SearchResponse>, StatusCode> {
    let query = SearchQuery::parse(raw.as_deref())?;
    let text = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let status = match query.status.as_deref() {
        Some(raw) => Some(parse_status(raw).ok_or(StatusCode::BAD_REQUEST)?),
        None => None,
    };
    if let (Some(after), Some(before)) = (query.after, query.before) {
        if after >= before {
            return Err(StatusCode::BAD_REQUEST);
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
        return Err(StatusCode::BAD_REQUEST);
    }
    let cursor = match query.cursor.as_deref() {
        Some(raw) => Some(decode_cursor(raw).ok_or(StatusCode::BAD_REQUEST)?),
        None => None,
    };
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let hits = strife_db::search_email(&pool, text, &filters, cursor, limit)
        .await
        .map_err(internal_error)?;
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

async fn facets(State(pool): State<PgPool>) -> Result<Json<FacetsResponse>, StatusCode> {
    let labels = strife_db::email_label_facets(&pool, FACET_LIMIT)
        .await
        .map_err(internal_error)?;
    let correspondents = strife_db::email_correspondent_facets(&pool, FACET_LIMIT)
        .await
        .map_err(internal_error)?;
    let years = strife_db::email_year_facets(&pool)
        .await
        .map_err(internal_error)?;
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
) -> Result<Json<MessageResponse>, StatusCode> {
    let record = strife_db::get_email_message(&pool, node_id)
        .await
        .map_err(internal_error)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let addresses = strife_db::list_email_addresses(&pool, node_id)
        .await
        .map_err(internal_error)?;
    let labels = strife_db::list_email_labels(&pool, node_id)
        .await
        .map_err(internal_error)?;
    let attachments = strife_db::list_email_attachments(&pool, node_id)
        .await
        .map_err(internal_error)?;
    let raw_headers = if query.include_raw_headers {
        Some(
            strife_db::list_email_headers(&pool, node_id)
                .await
                .map_err(internal_error)?
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
        body_html: record.body_html,
        preview_text: record.preview_text,
        thread_group_id: record.thread_group_id,
        duplicate_group_id: record.duplicate_group_id,
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
        warnings: record.warnings,
        raw_headers,
    }))
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
