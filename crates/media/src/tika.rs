use std::{path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use futures_util::TryStreamExt;
use reqwest::{Body, Client, header};
use serde_json::Value;
use tokio_util::io::ReaderStream;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Complete Apache Tika metadata plus normalized document properties.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TikaResult {
    pub title: Option<String>,
    pub author: Option<String>,
    pub creation_date: Option<String>,
    pub modification_date: Option<String>,
    pub page_count: Option<i32>,
    pub word_count: Option<i64>,
    pub warnings: Vec<String>,
    pub raw_payload: Value,
}

/// Sends a document to Apache Tika's `/meta` endpoint with default safety limits.
///
/// # Errors
///
/// Returns an error for file, HTTP, size-limit, or malformed JSON failures. Oversized responses
/// fail atomically rather than returning truncated metadata.
pub async fn extract_tika(path: &Path, tika_url: &str) -> Result<TikaResult> {
    extract_tika_with_limits(path, tika_url, DEFAULT_TIMEOUT, DEFAULT_MAX_RESPONSE_BYTES).await
}

/// Sends a document to Apache Tika with caller-specified timeout and response ceiling.
///
/// # Errors
///
/// Returns an error unless a complete successful JSON response can be returned.
pub async fn extract_tika_with_limits(
    path: &Path,
    tika_url: &str,
    request_timeout: Duration,
    max_response_bytes: usize,
) -> Result<TikaResult> {
    let file = tokio::fs::File::open(path)
        .await
        .context("open document for Tika")?;
    let client = Client::builder()
        .timeout(request_timeout)
        .build()
        .context("build Tika HTTP client")?;
    let response = client
        .put(format!("{}/meta", tika_url.trim_end_matches('/')))
        .header(header::ACCEPT, "application/json")
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::wrap_stream(ReaderStream::new(file)))
        .send()
        .await
        .context("send document to Tika")?
        .error_for_status()
        .context("Tika returned an unsuccessful status")?;

    let mut payload = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.try_next().await.context("read Tika response")? {
        if payload.len().saturating_add(chunk.len()) > max_response_bytes {
            bail!("Tika response exceeded {max_response_bytes} bytes");
        }
        payload.extend_from_slice(&chunk);
    }
    parse_tika_payload(serde_json::from_slice(&payload).context("parse Tika JSON")?)
}

fn parse_tika_payload(raw_payload: Value) -> Result<TikaResult> {
    let metadata = match &raw_payload {
        Value::Object(metadata) => metadata,
        Value::Array(records) => records
            .first()
            .and_then(Value::as_object)
            .context("Tika JSON array did not contain a metadata object")?,
        _ => bail!("Tika JSON was not a metadata object"),
    };
    let title = text(metadata, &["dc:title", "title"]);
    let author = text(metadata, &["dc:creator", "Author", "meta:author"]);
    let creation_date = text(metadata, &["dcterms:created", "Creation-Date", "created"]);
    let modification_date = text(metadata, &["dcterms:modified", "Last-Modified", "modified"]);
    let page_count = integer(metadata, &["xmpTPg:NPages", "meta:page-count"])
        .and_then(|value| i32::try_from(value).ok());
    let word_count = integer(metadata, &["meta:word-count", "Word-Count"]);
    let mut warnings = Vec::new();
    if page_count == Some(0) {
        warnings.push("document page count is zero".to_owned());
    }
    if title.is_none() && author.is_none() {
        warnings.push("document title and author are missing".to_owned());
    }
    Ok(TikaResult {
        title,
        author,
        creation_date,
        modification_date,
        page_count,
        word_count,
        warnings,
        raw_payload,
    })
}

fn value<'a>(metadata: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| metadata.get(*key))
}

fn text(metadata: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    value(metadata, keys).and_then(|value| match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Array(values) => values.iter().find_map(Value::as_str).map(str::to_owned),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn integer(metadata: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<i64> {
    value(metadata, keys).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
            .or_else(|| value.as_array()?.first()?.as_str()?.parse::<i64>().ok())
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use axum::{
        Router,
        body::Bytes,
        http::{HeaderMap, Method},
        routing::put,
    };
    use serde_json::{Value, json};
    use uuid::Uuid;

    use super::extract_tika;

    struct Fixture(PathBuf);

    impl Fixture {
        fn write(name: &str, bytes: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!("strife-tika-{}-{name}", Uuid::new_v4()));
            fs::write(&path, bytes).expect("write document fixture");
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    async fn metadata(method: Method, headers: HeaderMap, body: Bytes) -> axum::Json<Value> {
        assert_eq!(method, Method::PUT);
        assert_eq!(
            headers.get("accept").and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        if body.starts_with(b"%PDF") {
            axum::Json(json!({
                "dc:title": "Annual report",
                "dc:creator": ["Ada Lovelace"],
                "xmpTPg:NPages": "12",
                "meta:word-count": "4200",
                "X-Custom-Raw-Field": "retained"
            }))
        } else {
            axum::Json(json!({
                "title": "Meeting notes",
                "Author": "Grace Hopper",
                "Creation-Date": "2026-01-02T03:04:05Z",
                "meta:page-count": "3"
            }))
        }
    }

    #[tokio::test]
    async fn sends_pdf_and_docx_and_preserves_complete_metadata() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock Tika");
        let address = listener.local_addr().expect("mock Tika address");
        let server = tokio::spawn(
            axum::serve(listener, Router::new().route("/meta", put(metadata))).into_future(),
        );
        let pdf = Fixture::write("sample.pdf", b"%PDF-1.7\n%%EOF");
        let docx = Fixture::write("sample.docx", b"PK\x03\x04mock-docx");

        let pdf_result = extract_tika(&pdf.0, &format!("http://{address}"))
            .await
            .expect("extract PDF metadata");
        assert_eq!(pdf_result.page_count, Some(12));
        assert_eq!(pdf_result.author.as_deref(), Some("Ada Lovelace"));
        assert_eq!(pdf_result.raw_payload["X-Custom-Raw-Field"], "retained");

        let docx_result = extract_tika(&docx.0, &format!("http://{address}"))
            .await
            .expect("extract DOCX metadata");
        assert_eq!(docx_result.title.as_deref(), Some("Meeting notes"));
        assert_eq!(docx_result.page_count, Some(3));
        server.abort();
    }
}
