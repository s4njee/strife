//! RFC 5322 / MIME parsing for archived `.eml` files.
//!
//! The adapter is pure: it accepts bounded bytes and returns typed values. It
//! performs no DNS resolution, no link fetching, no remote-image loading, and
//! never executes an attachment. Rendering safety is a separate concern — this
//! module only produces text and descriptors.

mod html;

use std::collections::HashMap;

use anyhow::{Result, bail};
use chrono::{DateTime, TimeZone, Utc};
use mail_parser::{
    Address, HeaderName, HeaderValue, Message, MessageParser, MimeHeaders, PartType,
};
use sha2::{Digest, Sha256};

pub use html::html_to_text;

/// Parser identity persisted with every message, so a parser upgrade can be
/// detected and scheduled for bounded reprocessing.
pub const EMAIL_PARSER_NAME: &str = "mail-parser";
pub const EMAIL_PARSER_VERSION: &str = "0.11.5";

/// RFC role an address was written under.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmailAddressKind {
    From,
    Sender,
    ReplyTo,
    To,
    Cc,
    Bcc,
}

impl EmailAddressKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::From => "from",
            Self::Sender => "sender",
            Self::ReplyTo => "reply_to",
            Self::To => "to",
            Self::Cc => "cc",
            Self::Bcc => "bcc",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedAddress {
    pub kind: EmailAddressKind,
    pub display_name: Option<String>,
    pub address: String,
}

/// One header in original order, with its original casing preserved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedHeader {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedAttachment {
    pub part_path: String,
    pub filename: Option<String>,
    pub media_type: String,
    pub disposition: Option<String>,
    pub content_id: Option<String>,
    pub transfer_encoding: Option<String>,
    pub decoded_size: Option<i64>,
    pub checksum_sha256: Option<String>,
    pub is_inline: bool,
    pub is_message: bool,
    pub warnings: Vec<String>,
}

/// Everything one parse produces.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedEmail {
    pub message_id: Option<String>,
    pub normalized_message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub subject: Option<String>,
    pub normalized_subject: Option<String>,
    pub sent_at: Option<DateTime<Utc>>,
    pub received_at: Option<DateTime<Utc>>,
    pub addresses: Vec<ParsedAddress>,
    pub headers: Vec<ParsedHeader>,
    pub labels: Vec<String>,
    pub provider_thread_id: Option<String>,
    pub body_text: String,
    pub body_html: Option<String>,
    pub preview_text: String,
    pub content_hash: String,
    pub attachments: Vec<ParsedAttachment>,
    pub warnings: Vec<String>,
    pub parser_name: &'static str,
    pub parser_version: &'static str,
}

/// Bounds applied while parsing one message.
///
/// Defaults are provisional starting values; Story 22.2 profiles them on Orion
/// before they are treated as policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmailParseLimits {
    pub max_source_bytes: usize,
    pub max_body_bytes: usize,
    pub max_preview_bytes: usize,
    pub max_headers: usize,
    pub max_attachments: usize,
    pub max_warnings: usize,
}

impl Default for EmailParseLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 64 * 1024 * 1024,
            max_body_bytes: 2 * 1024 * 1024,
            max_preview_bytes: 512,
            max_headers: 512,
            max_attachments: 256,
            max_warnings: 64,
        }
    }
}

/// Returns whether a detected MIME type is an RFC 5322 message.
#[must_use]
pub fn is_rfc822_mime(mime: &str) -> bool {
    matches!(mime, "message/rfc822" | "message/global")
}

/// Confirms a byte buffer looks like an RFC 5322 message.
///
/// `file`'s MIME detection is unreliable for `.eml` exports, so extraction
/// confirms the shape from bytes rather than trusting the extension or the
/// upload-supplied type. The check is deliberately structural: at least one
/// well-formed header line before a blank line, including one header that
/// actually identifies a message.
#[must_use]
pub fn looks_like_rfc822(bytes: &[u8]) -> bool {
    // Only the header block is inspected. Header field names are ASCII, but a
    // body may be any charset — a ten-year archive contains plenty of latin-1 —
    // so requiring the whole sniffing window to be UTF-8 would reject valid
    // messages purely for their body encoding.
    let window = &bytes[..bytes.len().min(8192)];
    let head = find_header_block(window);
    let Ok(text) = std::str::from_utf8(head) else {
        return false;
    };
    let mut named = false;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            continue; // folded continuation
        }
        let Some((name, _)) = line.split_once(':') else {
            return false;
        };
        if name.is_empty() || !name.bytes().all(|byte| (33..=126).contains(&byte)) {
            return false;
        }
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "from" | "to" | "subject" | "date" | "message-id" | "received" | "mime-version"
        ) {
            named = true;
        }
    }
    named
}

/// Returns the bytes preceding the header/body separator.
///
/// Falls back to the whole window when no separator is present, since a
/// truncated sniffing window may legitimately contain headers only.
fn find_header_block(window: &[u8]) -> &[u8] {
    let crlf = window
        .windows(4)
        .position(|quad| quad == b"\r\n\r\n")
        .map(|at| &window[..at]);
    let lf = window
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .map(|at| &window[..at]);
    match (crlf, lf) {
        (Some(a), Some(b)) => {
            if a.len() <= b.len() {
                a
            } else {
                b
            }
        }
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => window,
    }
}

/// Parses one message into its normalized projection.
///
/// # Errors
///
/// Returns an error when the input exceeds `max_source_bytes`, is not an RFC
/// 5322 message, or cannot be parsed at all.
#[allow(clippy::too_many_lines)]
pub fn parse_email(bytes: &[u8], limits: EmailParseLimits) -> Result<ParsedEmail> {
    if bytes.len() > limits.max_source_bytes {
        bail!(
            "email source size limit exceeded: {} bytes is greater than {}",
            bytes.len(),
            limits.max_source_bytes
        );
    }
    if !looks_like_rfc822(bytes) {
        bail!("input is not an RFC 5322 message");
    }
    let Some(message) = MessageParser::default().parse(bytes) else {
        bail!("message could not be parsed");
    };

    let mut warnings = Vec::new();
    let paths = part_paths(&message);

    let (body_text, body_html) = select_bodies(&message, limits, &mut warnings);
    let preview_text = preview(&body_text, limits.max_preview_bytes);
    let headers = collect_headers(&message, limits, &mut warnings);
    let addresses = collect_addresses(&message);
    let (labels, provider_thread_id) = gmail_context(&headers);
    let attachments = collect_attachments(&message, &paths, limits, &mut warnings);

    let subject = message.subject().map(ToOwned::to_owned);
    let normalized_subject = subject.as_deref().map(normalize_subject);
    let message_id = message.message_id().map(ToOwned::to_owned);
    let normalized_message_id = message_id.as_deref().map(normalize_message_id);

    // A Date header that is present but unparseable stays null and warns; it
    // is never replaced with ingestion time, which would invent a plausible
    // date the sender never wrote.
    let sent_at = message.date().and_then(to_utc);
    if sent_at.is_none() && (message.date().is_some() || header_present(&headers, "date")) {
        warnings.push("date header could not be parsed and was left unset".to_owned());
    }
    // Only trusted when the trace header parsed cleanly; never falls back to
    // ingestion time, which would silently invent a plausible-looking date.
    let received_at = message
        .received_all()
        .filter_map(mail_parser::Received::date)
        .find_map(|date| to_utc(&date));

    let content_hash = canonical_hash(&addresses, normalized_subject.as_deref(), &body_text);
    warnings.truncate(limits.max_warnings);

    Ok(ParsedEmail {
        message_id,
        normalized_message_id,
        in_reply_to: first_id(message.in_reply_to()),
        references: all_ids(message.references()),
        subject,
        normalized_subject,
        sent_at,
        received_at,
        addresses,
        headers,
        labels,
        provider_thread_id,
        body_text,
        body_html,
        preview_text,
        content_hash,
        attachments,
        warnings,
        parser_name: EMAIL_PARSER_NAME,
        parser_version: EMAIL_PARSER_VERSION,
    })
}

/// Maps every part id to a dotted MIME path such as `1.2`.
fn part_paths(message: &Message<'_>) -> HashMap<usize, String> {
    let mut paths = HashMap::new();
    let mut stack = vec![(0usize, String::from("1"))];
    while let Some((id, path)) = stack.pop() {
        let Some(part) = message.parts.get(id) else {
            continue;
        };
        paths.insert(id, path.clone());
        if let PartType::Multipart(children) = &part.body {
            for (index, child) in children.iter().enumerate() {
                stack.push((*child as usize, format!("{path}.{}", index + 1)));
            }
        }
    }
    paths
}

fn select_bodies(
    message: &Message<'_>,
    limits: EmailParseLimits,
    warnings: &mut Vec<String>,
) -> (String, Option<String>) {
    let html = message.body_html(0).map(std::borrow::Cow::into_owned);
    // A usable text/plain alternative is preferred for search; the HTML
    // alternative is still retained for the reader.
    let plain = message
        .body_text(0)
        .map(std::borrow::Cow::into_owned)
        .filter(|value| !value.trim().is_empty());

    let mut text = match plain {
        Some(value) => value,
        None => match html.as_deref() {
            Some(markup) => {
                let (converted, mut html_warnings) = html_to_text(markup);
                warnings.append(&mut html_warnings);
                converted
            }
            None => String::new(),
        },
    };
    if text.trim().is_empty() {
        // A message whose MIME structure is broken — an unterminated multipart
        // boundary, say — has no assembled body, but its parts still hold
        // readable text. Recovering it is better than indexing an empty body
        // and losing the message to a malformed boundary.
        if let Some(recovered) = recover_body(&message.parts) {
            warnings.push(
                "message structure is malformed; body text was recovered from its parts".to_owned(),
            );
            text = recovered;
        }
    }
    text = normalize_text(&text);
    if text.len() > limits.max_body_bytes {
        truncate_chars(&mut text, limits.max_body_bytes);
        warnings.push(format!(
            "body text limit exceeded and was truncated to {} bytes",
            limits.max_body_bytes
        ));
    }
    (text, html)
}

/// Salvages readable text from a message whose part tree could not be
/// assembled into a body.
fn recover_body(parts: &[mail_parser::MessagePart<'_>]) -> Option<String> {
    let mut recovered = String::new();
    for part in parts {
        match &part.body {
            PartType::Text(text) => {
                if !text.trim().is_empty() {
                    recovered.push_str(text);
                    recovered.push('\n');
                }
            }
            PartType::Html(markup) => {
                let (converted, _) = html_to_text(markup);
                if !converted.trim().is_empty() {
                    recovered.push_str(&converted);
                    recovered.push('\n');
                }
            }
            _ => {}
        }
    }
    (!recovered.trim().is_empty()).then_some(recovered)
}

/// Normalizes line endings and trims trailing whitespace per line.
fn normalize_text(raw: &str) -> String {
    raw.replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn truncate_chars(value: &mut String, max_bytes: usize) {
    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}

fn preview(body: &str, max_bytes: usize) -> String {
    let single_line = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = single_line;
    truncate_chars(&mut preview, max_bytes);
    preview
}

fn collect_headers(
    message: &Message<'_>,
    limits: EmailParseLimits,
    warnings: &mut Vec<String>,
) -> Vec<ParsedHeader> {
    let mut headers: Vec<ParsedHeader> = message
        .headers_raw()
        .map(|(name, value)| ParsedHeader {
            name: name.trim().to_owned(),
            value: value.trim().to_owned(),
        })
        .collect();
    if headers.len() > limits.max_headers {
        warnings.push(format!(
            "header count limit exceeded; kept the first {}",
            limits.max_headers
        ));
        headers.truncate(limits.max_headers);
    }
    headers
}

fn header_present(headers: &[ParsedHeader], name: &str) -> bool {
    headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(name))
}

fn header_value<'a>(headers: &'a [ParsedHeader], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str())
}

/// Collects addresses from every address-valued header, in document order.
///
/// The typed accessors (`message.to()` and friends) return a single header, so
/// a message carrying two `To:` lines — which real archives do — would silently
/// lose every recipient but the last. Walking the header list preserves them
/// all, and keeps the order the sender actually wrote.
fn collect_addresses(message: &Message<'_>) -> Vec<ParsedAddress> {
    let mut out = Vec::new();
    for header in message.headers() {
        let kind = match &header.name {
            HeaderName::From => EmailAddressKind::From,
            HeaderName::Sender => EmailAddressKind::Sender,
            HeaderName::ReplyTo => EmailAddressKind::ReplyTo,
            HeaderName::To => EmailAddressKind::To,
            HeaderName::Cc => EmailAddressKind::Cc,
            HeaderName::Bcc => EmailAddressKind::Bcc,
            _ => continue,
        };
        if let HeaderValue::Address(address) = &header.value {
            push_addresses(kind, address, &mut out);
        }
    }
    out
}

fn push_addresses(kind: EmailAddressKind, address: &Address<'_>, out: &mut Vec<ParsedAddress>) {
    match address {
        Address::List(items) => {
            for item in items {
                push_one(kind, item.name.as_deref(), item.address.as_deref(), out);
            }
        }
        // Group syntax (`Team: a@x, b@x;`) keeps its members, not the label.
        Address::Group(groups) => {
            for group in groups {
                for item in &group.addresses {
                    push_one(kind, item.name.as_deref(), item.address.as_deref(), out);
                }
            }
        }
    }
}

fn push_one(
    kind: EmailAddressKind,
    name: Option<&str>,
    address: Option<&str>,
    out: &mut Vec<ParsedAddress>,
) {
    let Some(address) = address else { return };
    let normalized = normalize_address(address);
    if normalized.is_empty() {
        return;
    }
    out.push(ParsedAddress {
        kind,
        display_name: name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        address: normalized,
    });
}

/// Lowercases the domain and preserves the local part.
///
/// Gmail-specific dot and plus-address rewriting is deliberately not applied:
/// two addresses that Gmail treats as one mailbox are still distinct as
/// written, and collapsing them would lose what the sender actually typed.
#[must_use]
pub fn normalize_address(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches(|c| c == '<' || c == '>').trim();
    match trimmed.rsplit_once('@') {
        Some((local, domain)) => format!("{local}@{}", domain.to_ascii_lowercase()),
        None => trimmed.to_owned(),
    }
}

/// Strips angle brackets and lowercases for comparison.
#[must_use]
pub fn normalize_message_id(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_ascii_lowercase()
}

/// Derives a thread-comparison subject without changing the displayed subject.
#[must_use]
pub fn normalize_subject(raw: &str) -> String {
    let mut value = raw.trim();
    loop {
        let lower = value.to_ascii_lowercase();
        let trimmed = ["re:", "fwd:", "fw:", "aw:", "sv:"]
            .iter()
            .find_map(|prefix| {
                lower
                    .starts_with(prefix)
                    .then(|| value[prefix.len()..].trim())
            });
        match trimmed {
            Some(next) => value = next,
            None => break,
        }
    }
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn gmail_context(headers: &[ParsedHeader]) -> (Vec<String>, Option<String>) {
    let mut labels: Vec<String> = header_value(headers, "x-gmail-labels")
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();
    labels.sort();
    labels.dedup();
    let thread = header_value(headers, "x-gm-thrid")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    (labels, thread)
}

fn collect_attachments(
    message: &Message<'_>,
    paths: &HashMap<usize, String>,
    limits: EmailParseLimits,
    warnings: &mut Vec<String>,
) -> Vec<ParsedAttachment> {
    let mut out = Vec::new();
    for id in &message.attachments {
        let index = *id as usize;
        let Some(part) = message.parts.get(index) else {
            warnings.push("attachment part referenced a missing index".to_owned());
            continue;
        };
        if out.len() >= limits.max_attachments {
            warnings.push(format!(
                "attachment count limit exceeded; kept the first {}",
                limits.max_attachments
            ));
            break;
        }
        let mut part_warnings = Vec::new();
        let media_type = part.content_type().map_or_else(
            || "application/octet-stream".to_owned(),
            |content| match content.c_subtype.as_deref() {
                Some(subtype) => format!("{}/{subtype}", content.c_type).to_ascii_lowercase(),
                None => content.c_type.to_ascii_lowercase(),
            },
        );
        let is_message = part.is_message();
        let (decoded_size, checksum) = match &part.body {
            PartType::Text(text) | PartType::Html(text) => {
                let bytes = text.as_bytes();
                (i64::try_from(bytes.len()).ok(), Some(digest(bytes)))
            }
            PartType::Binary(data) | PartType::InlineBinary(data) => {
                (i64::try_from(data.len()).ok(), Some(digest(data)))
            }
            // A nested message is described but not flattened here; Story 21.1
            // decides whether its bytes are materialized at all.
            PartType::Message(_) | PartType::Multipart(_) => (None, None),
        };
        if part.is_encoding_problem {
            part_warnings.push("part reported a transfer-encoding problem".to_owned());
        }
        let disposition = part
            .content_disposition()
            .map(|value| value.c_type.to_ascii_lowercase());
        let is_inline = disposition.as_deref() == Some("inline") || part.content_id().is_some();

        out.push(ParsedAttachment {
            // Filenames are display values only. They are never used as a
            // filesystem path; storage keys derive from part identity instead.
            part_path: paths
                .get(&index)
                .cloned()
                .unwrap_or_else(|| format!("part-{index}")),
            filename: part
                .attachment_name()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            media_type,
            disposition,
            content_id: part.content_id().map(normalize_message_id),
            transfer_encoding: part.content_transfer_encoding().map(ToOwned::to_owned),
            decoded_size,
            checksum_sha256: checksum,
            is_inline,
            is_message,
            warnings: part_warnings,
        });
    }
    out
}

fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Hashes the parts of a message that identify it independently of transport.
///
/// Trace headers differ between copies of the same message, so they are
/// excluded: this is what lets two exports of one message group as duplicates
/// when neither carries a `Message-ID`.
fn canonical_hash(
    addresses: &[ParsedAddress],
    normalized_subject: Option<&str>,
    body_text: &str,
) -> String {
    let from = addresses
        .iter()
        .find(|address| address.kind == EmailAddressKind::From)
        .map_or("", |address| address.address.as_str());
    let mut hasher = Sha256::new();
    hasher.update(from.as_bytes());
    hasher.update([0]);
    hasher.update(normalized_subject.unwrap_or_default().as_bytes());
    hasher.update([0]);
    hasher.update(body_text.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn to_utc(value: &mail_parser::DateTime) -> Option<DateTime<Utc>> {
    value
        .is_valid()
        .then(|| Utc.timestamp_opt(value.to_timestamp(), 0).single())
        .flatten()
}

fn first_id(value: &HeaderValue<'_>) -> Option<String> {
    match value {
        HeaderValue::Text(text) => Some(normalize_message_id(text)),
        HeaderValue::TextList(list) => list.first().map(|text| normalize_message_id(text)),
        _ => None,
    }
}

fn all_ids(value: &HeaderValue<'_>) -> Vec<String> {
    match value {
        HeaderValue::Text(text) => vec![normalize_message_id(text)],
        HeaderValue::TextList(list) => list.iter().map(|text| normalize_message_id(text)).collect(),
        _ => Vec::new(),
    }
}
