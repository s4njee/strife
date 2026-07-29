use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::get,
};
use sqlx::PgPool;
use strife_storage::{StorageBackend, StorageKey};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

#[derive(Clone)]
struct FileState {
    pool: PgPool,
    storage: Arc<dyn StorageBackend>,
}

/// Builds the original-file download router.
pub fn router(pool: PgPool, storage: Arc<dyn StorageBackend>) -> Router {
    Router::new()
        .route("/api/files/{id}/download", get(download_file))
        .with_state(FileState { pool, storage })
}

async fn download_file(
    State(state): State<FileState>,
    Path(node_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    try_download(&state, node_id, &headers)
        .await
        .unwrap_or_else(DownloadError::into_response)
}

async fn try_download(
    state: &FileState,
    node_id: Uuid,
    headers: &HeaderMap,
) -> Result<Response, DownloadError> {
    let file = strife_db::get_download_file(&state.pool, node_id)
        .await
        .map_err(|_| DownloadError::Internal)?
        .ok_or(DownloadError::NotFound)?;
    let object_id = Uuid::parse_str(&file.storage_key).map_err(|_| DownloadError::Internal)?;
    let total = u64::try_from(file.byte_size).map_err(|_| DownloadError::Internal)?;
    let requested_range = headers
        .get(header::RANGE)
        .map(|value| parse_range(value, total).map_err(|()| DownloadError::Range(total)))
        .transpose()?;
    let (status, start, length) = requested_range.map_or((StatusCode::OK, 0, total), |range| {
        (StatusCode::PARTIAL_CONTENT, range.start, range.length())
    });
    let reader = if requested_range.is_some() {
        state
            .storage
            .get_range(StorageKey::original(object_id), start, length)
            .await
    } else {
        state
            .storage
            .get_stream(StorageKey::original(object_id))
            .await
    }
    .map_err(|_| DownloadError::NotFound)?;
    let body = Body::from_stream(ReaderStream::new(reader));
    let mut response = Response::builder()
        .status(status)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, length.to_string())
        .header(
            header::CONTENT_TYPE,
            file.mime_type
                .as_deref()
                .unwrap_or("application/octet-stream"),
        )
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}\"",
                safe_filename(&file.display_name)
            ),
        );
    if let Some(range) = requested_range {
        response = response.header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", range.start, range.end, total),
        );
    }
    Ok(response
        .body(body)
        .unwrap_or_else(|_| status_response(StatusCode::INTERNAL_SERVER_ERROR)))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DownloadError {
    NotFound,
    Range(u64),
    Internal,
}

impl DownloadError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => status_response(StatusCode::NOT_FOUND),
            Self::Range(total) => range_not_satisfiable(total),
            Self::Internal => status_response(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ByteRange {
    start: u64,
    end: u64,
}

impl ByteRange {
    const fn length(self) -> u64 {
        self.end - self.start + 1
    }
}

fn parse_range(value: &HeaderValue, total: u64) -> Result<ByteRange, ()> {
    let value = value
        .to_str()
        .ok()
        .and_then(|value| value.strip_prefix("bytes="))
        .filter(|value| !value.contains(','))
        .ok_or(())?;
    let (start, end) = value.split_once('-').ok_or(())?;
    let range = if start.is_empty() {
        let suffix = end
            .parse::<u64>()
            .ok()
            .filter(|suffix| *suffix > 0)
            .ok_or(())?;
        ByteRange {
            start: total.saturating_sub(suffix.min(total)),
            end: total.saturating_sub(1),
        }
    } else {
        let start = start.parse::<u64>().map_err(|_| ())?;
        let end = if end.is_empty() {
            total.saturating_sub(1)
        } else {
            end.parse::<u64>()
                .map_err(|_| ())?
                .min(total.saturating_sub(1))
        };
        ByteRange { start, end }
    };
    if total == 0 || range.start > range.end || range.start >= total {
        return Err(());
    }
    Ok(range)
}

fn safe_filename(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            '\r' | '\n' | '\0' | '"' | '\\' => '_',
            other => other,
        })
        .collect()
}

fn range_not_satisfiable(total: u64) -> Response {
    Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(header::CONTENT_RANGE, format!("bytes */{total}"))
        .body(Body::empty())
        .unwrap_or_else(|_| status_response(StatusCode::INTERNAL_SERVER_ERROR))
}

fn status_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .body(Body::empty())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::{ByteRange, parse_range, safe_filename};

    #[test]
    fn parses_closed_open_and_suffix_ranges() {
        assert_eq!(
            parse_range(&HeaderValue::from_static("bytes=2-5"), 10).ok(),
            Some(ByteRange { start: 2, end: 5 })
        );
        assert_eq!(
            parse_range(&HeaderValue::from_static("bytes=7-"), 10).ok(),
            Some(ByteRange { start: 7, end: 9 })
        );
        assert_eq!(
            parse_range(&HeaderValue::from_static("bytes=-3"), 10).ok(),
            Some(ByteRange { start: 7, end: 9 })
        );
    }

    #[test]
    fn rejects_invalid_or_multiple_ranges_and_sanitizes_names() {
        assert!(parse_range(&HeaderValue::from_static("bytes=10-12"), 10).is_err());
        assert!(parse_range(&HeaderValue::from_static("bytes=0-1,3-4"), 10).is_err());
        assert_eq!(safe_filename("unsafe\r\n\"name"), "unsafe___name");
    }
}
