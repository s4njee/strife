//! Renders archived HTML email safe to display.
//!
//! Archived mail is hostile input that happens to be a decade old: it contains
//! tracking pixels, script tags that were live when the message was sent, and
//! markup written for renderers that no longer exist. Sanitizing is done here,
//! server-side, so the browser never receives the original bytes at all.
//!
//! This is the inner of two defences. The outer one is the reader's sandboxed
//! frame, which withholds script execution and same-origin access regardless of
//! what survives this pass. Neither layer is asked to be sufficient alone.
//!
//! Three properties are deliberate:
//!
//! - **Nothing loads from the network.** Every remote subresource is stripped
//!   and counted, so "this message wanted to contact 4 hosts" is something the
//!   reader can state rather than something that silently happened. A tracking
//!   pixel that fires on open has already told the sender the archive was read.
//! - **CSS is filtered by property, not passed through.** Inline styles carry
//!   most of an archived message's layout, so dropping them wholesale would make
//!   a decade of mail unreadable. Values that can fetch, position, or escape are
//!   rejected instead.
//! - **`cid:` resolves only within its own message.** An inline reference names a
//!   MIME part of the message being read; it can never be steered at another
//!   message's parts or at an arbitrary storage key.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, PoisonError},
};

use ammonia::{Builder, UrlRelative};
use uuid::Uuid;

/// Upper bound on distinct hosts reported to the reader. A message that
/// references hundreds of hosts is a tracking artifact; naming the first few is
/// enough for a user to decide, and the full list is not worth the payload.
const MAX_REPORTED_HOSTS: usize = 8;

/// One inline MIME part a `cid:` reference is allowed to resolve to.
#[derive(Clone, Copy, Debug)]
pub struct InlinePart<'a> {
    /// `Content-ID` as parsed, with any angle brackets already removed.
    pub content_id: &'a str,
    /// MIME part path within the containing message.
    pub part_path: &'a str,
}

/// How one message body should be sanitized.
#[derive(Clone, Copy, Debug)]
pub struct SanitizeOptions<'a> {
    /// The message being read. Inline references resolve only within it.
    pub node_id: Uuid,
    /// Parts a `cid:` reference is allowed to name.
    pub inline: &'a [InlinePart<'a>],
    /// Whether to keep remote image sources.
    ///
    /// False by default and true only when the reader has explicitly asked to
    /// reveal them. Remote URLs are removed rather than hidden client-side,
    /// because a tracking pixel that reaches the browser has already fired: the
    /// bytes must not leave the server until the user chooses.
    pub allow_remote_images: bool,
}

/// Outcome of sanitizing one message body.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SanitizedHtml {
    /// Markup safe to place inside the reader's sandboxed frame.
    pub html: String,
    /// How many remote subresources were neutralized.
    pub blocked_remote_count: usize,
    /// Distinct hosts the message tried to contact, bounded and sorted.
    pub blocked_hosts: Vec<String>,
    /// Part paths successfully resolved from `cid:` references.
    pub inline_parts: Vec<String>,
    /// Human-readable notes worth surfacing in the reader.
    pub warnings: Vec<String>,
}

/// Elements permitted to survive. Structural and textual only: no `script`,
/// `style`, `iframe`, `object`, `embed`, `form`, `input`, `svg`, `math`,
/// `link`, `meta`, or `base` appears here, so scripting, submission, remote
/// stylesheet loading, plugin content, and meta-refresh redirects have no
/// element to attach to.
const ALLOWED_TAGS: &[&str] = &[
    "a",
    "abbr",
    "b",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "dd",
    "del",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "font",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "ol",
    "p",
    "pre",
    "q",
    "s",
    "samp",
    "small",
    "span",
    "strike",
    "strong",
    "sub",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "tt",
    "u",
    "ul",
    "wbr",
];

/// Elements removed together with their text. Without this, a `<script>` body
/// would be discarded as an element but its source code would remain as visible
/// text in the message.
const DROP_WITH_CONTENT: &[&str] = &["script", "style", "title", "noscript", "template", "iframe"];

/// CSS properties allowed to survive in a `style` attribute. Layout and colour
/// only. Anything that can fetch (`background`, `src`), escape the frame
/// (`position`, `z-index`), or affect the containing document is absent.
const ALLOWED_STYLE_PROPERTIES: &[&str] = &[
    "background-color",
    "border",
    "border-bottom",
    "border-bottom-color",
    "border-bottom-style",
    "border-bottom-width",
    "border-collapse",
    "border-color",
    "border-left",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-radius",
    "border-right",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-spacing",
    "border-style",
    "border-top",
    "border-top-color",
    "border-top-style",
    "border-top-width",
    "border-width",
    "color",
    "font-family",
    "font-size",
    "font-style",
    "font-variant",
    "font-weight",
    "height",
    "letter-spacing",
    "line-height",
    "list-style-type",
    "margin",
    "margin-bottom",
    "margin-left",
    "margin-right",
    "margin-top",
    "max-height",
    "max-width",
    "min-height",
    "min-width",
    "padding",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "padding-top",
    "text-align",
    "text-decoration",
    "text-indent",
    "text-transform",
    "vertical-align",
    "white-space",
    "width",
    "word-break",
    "word-wrap",
];

/// Substrings that disqualify a CSS declaration outright. `url(` and `image-set`
/// fetch; `expression` and `javascript:` execute on legacy engines; `@import`
/// pulls a remote stylesheet; `\` is a CSS escape that can spell any of the
/// preceding; `/*` can hide them from a naive reader of the value.
const CSS_POISON: &[&str] = &[
    "url(",
    "image-set",
    "expression",
    "javascript:",
    "vbscript:",
    "@import",
    "\\",
    "/*",
];

/// Schemes ammonia is told to tolerate so that the attribute filter below gets
/// to see them. This is *not* the security boundary: ammonia's own check runs
/// before the filter, so anything the filter must inspect — `cid:` to rewrite,
/// `data:` to classify — has to survive this list first. `filter_link_target`
/// applies the real policy afterwards.
const FILTERABLE_SCHEMES: &[&str] = &["http", "https", "mailto", "tel", "cid", "data"];

/// Schemes a link may actually navigate to.
const SAFE_LINK_SCHEMES: &[&str] = &["http://", "https://", "mailto:", "tel:"];

/// Collected side effects of one sanitizing pass.
///
/// The attribute filter ammonia calls must be `Send + Sync`, so findings are
/// accumulated behind a lock rather than returned from the closure.
#[derive(Default)]
struct Findings {
    blocked_remote_count: usize,
    blocked_hosts: HashSet<String>,
    inline_parts: Vec<String>,
    unresolved_cid: usize,
}

/// Sanitizes one HTML body for display in the message reader.
///
/// `options.inline` lists the MIME parts of *this* message; a `cid:` reference
/// matching none of them is dropped rather than guessed at.
#[must_use]
pub fn sanitize_email_html(html: &str, options: &SanitizeOptions<'_>) -> SanitizedHtml {
    let node_id = options.node_id;
    let allow_remote_images = options.allow_remote_images;
    // Content-ID comparison is case-insensitive in practice: senders and their
    // own generators disagree about case far more often than they collide.
    let lookup: HashMap<String, String> = options
        .inline
        .iter()
        .map(|part| {
            (
                normalize_content_id(part.content_id),
                part.part_path.to_owned(),
            )
        })
        .collect();
    let findings = Arc::new(Mutex::new(Findings::default()));
    let filter_findings = Arc::clone(&findings);

    let mut tag_attributes: HashMap<&str, HashSet<&str>> = HashMap::new();
    tag_attributes.insert("a", ["href", "title", "name"].into_iter().collect());
    tag_attributes.insert(
        "img",
        ["src", "alt", "width", "height", "title"]
            .into_iter()
            .collect(),
    );
    tag_attributes.insert("td", ["colspan", "rowspan", "align", "valign"].into());
    tag_attributes.insert("th", ["colspan", "rowspan", "align", "valign"].into());
    tag_attributes.insert("col", ["span", "width"].into());
    tag_attributes.insert("colgroup", ["span", "width"].into());
    tag_attributes.insert(
        "table",
        ["align", "width", "cellpadding", "cellspacing"].into(),
    );
    tag_attributes.insert("ol", ["start", "type"].into());
    tag_attributes.insert("font", ["color", "face", "size"].into());

    let mut builder = Builder::default();
    builder
        .tags(ALLOWED_TAGS.iter().copied().collect())
        .clean_content_tags(DROP_WITH_CONTENT.iter().copied().collect())
        .tag_attributes(tag_attributes)
        // `class` and `dir` are inert and preserve some layout intent; `style`
        // is admitted only because the filter below rewrites its value.
        .generic_attributes(["class", "dir", "style", "align"].into_iter().collect())
        .url_schemes(FILTERABLE_SCHEMES.iter().copied().collect())
        // Relative URLs are passed through to the attribute filter rather than
        // denied here, because the filter needs to *emit* one: an inline part
        // resolves to a same-origin Strife path. Relative URLs arriving from the
        // message are rejected by the filter instead.
        .url_relative(UrlRelative::PassThrough)
        .link_rel(Some("noopener noreferrer nofollow"))
        .strip_comments(true)
        .attribute_filter(move |element, attribute, value| {
            filter_attribute(
                &filter_findings,
                &lookup,
                node_id,
                allow_remote_images,
                element,
                attribute,
                value,
            )
        });

    let cleaned = builder.clean(html).to_string();

    let findings = findings.lock().unwrap_or_else(PoisonError::into_inner);
    let mut blocked_hosts: Vec<String> = findings.blocked_hosts.iter().cloned().collect();
    blocked_hosts.sort();
    let truncated = blocked_hosts.len().saturating_sub(MAX_REPORTED_HOSTS);
    blocked_hosts.truncate(MAX_REPORTED_HOSTS);

    let mut warnings = Vec::new();
    if truncated > 0 {
        warnings.push(format!("{truncated} further remote hosts not listed"));
    }
    if findings.unresolved_cid > 0 {
        warnings.push(format!(
            "{} inline reference(s) named a part this message does not contain",
            findings.unresolved_cid
        ));
    }

    SanitizedHtml {
        html: cleaned,
        blocked_remote_count: findings.blocked_remote_count,
        blocked_hosts,
        inline_parts: findings.inline_parts.clone(),
        warnings,
    }
}

/// Strips the angle brackets and case distinctions around a `Content-ID`.
fn normalize_content_id(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_ascii_lowercase()
}

/// Decides the fate of one attribute that survived the tag allowlist.
#[allow(clippy::fn_params_excessive_bools)]
fn filter_attribute<'a>(
    findings: &Mutex<Findings>,
    lookup: &HashMap<String, String>,
    node_id: Uuid,
    allow_remote_images: bool,
    element: &str,
    attribute: &str,
    value: &'a str,
) -> Option<Cow<'a, str>> {
    // Event handlers reach here only if a future allowlist edit lets them; the
    // check costs nothing and fails closed if that happens.
    if attribute.len() > 2 && attribute.as_bytes()[..2].eq_ignore_ascii_case(b"on") {
        return None;
    }
    match (element, attribute) {
        // Two unrelated reasons to drop, sharing one arm:
        //
        // - any attribute of a `<style>` element, which the tag allowlist
        //   already removes — kept as a guard against a future allowlist edit;
        // - a link's `title`, because link text in archived mail is frequently
        //   a lie ("click here", or a display URL that differs from the
        //   target). The reader replaces it on load with the real destination,
        //   so a sender-written tooltip is discarded rather than trusted.
        ("style", _) | ("a", "title") => None,
        (_, "style") => {
            let filtered = filter_inline_style(value);
            (!filtered.is_empty()).then_some(Cow::Owned(filtered))
        }
        // A blocked image drops its `src` entirely rather than emitting an
        // empty one: `src=""` makes some browsers re-request the containing
        // page. An `img` with no `src` renders as its alt text, which is
        // exactly the placeholder the reader wants.
        ("img", "src") => {
            let rewritten =
                rewrite_image_source(findings, lookup, node_id, allow_remote_images, value);
            (!rewritten.is_empty()).then_some(Cow::Owned(rewritten))
        }
        ("a", "href") => filter_link_target(value),
        _ => Some(Cow::Borrowed(value)),
    }
}

/// Applies the real link policy.
///
/// Only absolute URLs in an explicitly safe scheme navigate. A relative URL is
/// rejected because an archived message carries no base: the browser would
/// resolve it against Strife's own origin, turning a decade-old link into a
/// request to the application. A protocol-relative `//host/path` is rejected for
/// the same reason it looks harmless — it inherits Strife's scheme.
fn filter_link_target(value: &str) -> Option<Cow<'_, str>> {
    let trimmed = value.trim();
    SAFE_LINK_SCHEMES
        .iter()
        .any(|scheme| strip_scheme(trimmed, scheme).is_some())
        .then(|| Cow::Owned(trimmed.to_owned()))
}

/// Rewrites an image source to something that cannot reach the network.
///
/// Remote sources become a marked placeholder rather than a removed attribute,
/// so the reader can show where an image *was* and offer to reveal it, instead
/// of silently collapsing the message's layout.
fn rewrite_image_source(
    findings: &Mutex<Findings>,
    lookup: &HashMap<String, String>,
    node_id: Uuid,
    allow_remote_images: bool,
    value: &str,
) -> String {
    let trimmed = value.trim();
    if let Some(reference) = strip_scheme(trimmed, "cid:") {
        let key = normalize_content_id(reference);
        if let Some(part_path) = lookup.get(&key) {
            if let Ok(mut findings) = findings.lock() {
                findings.inline_parts.push(part_path.clone());
            }
            // Same-origin, authenticated, and scoped to this message's node: a
            // rewritten reference cannot name another message's part.
            return format!("/api/email/messages/{node_id}/parts/{part_path}");
        }
        if let Ok(mut findings) = findings.lock() {
            findings.unresolved_cid += 1;
        }
        return String::new();
    }
    // `data:` images cannot reach the network, but they can carry SVG, which is
    // active content. Only inert raster types are kept.
    if let Some(rest) = strip_scheme(trimmed, "data:") {
        let inert = [
            "image/png",
            "image/jpeg",
            "image/gif",
            "image/webp",
            "image/bmp",
        ]
        .iter()
        .any(|media| rest.len() > media.len() && rest[..media.len()].eq_ignore_ascii_case(media));
        if inert {
            return trimmed.to_owned();
        }
        return String::new();
    }
    // Only sources that could actually reach a server are reported as blocked.
    // A relative `src` in an archived message resolves against nothing and is
    // simply broken; counting it would inflate the "this message contacted N
    // hosts" warning that the reader asks users to act on.
    let is_remote = trimmed.starts_with("//")
        || strip_scheme(trimmed, "http://").is_some()
        || strip_scheme(trimmed, "https://").is_some();
    if is_remote {
        if let Ok(mut findings) = findings.lock() {
            findings.blocked_remote_count += 1;
            if let Some(host) = host_of(trimmed) {
                findings.blocked_hosts.insert(host);
            }
        }
        // Still counted and reported when revealed, so the reader can keep
        // showing which hosts the message contacts after the user consents.
        if allow_remote_images {
            return trimmed.to_owned();
        }
    }
    String::new()
}

/// Matches a URL scheme prefix without allocating or lowercasing the whole URL.
fn strip_scheme<'a>(value: &'a str, scheme: &str) -> Option<&'a str> {
    (value.len() >= scheme.len() && value[..scheme.len()].eq_ignore_ascii_case(scheme))
        .then(|| &value[scheme.len()..])
}

/// Extracts a host for reporting. Best effort: this is used to describe a
/// blocked request, never to decide whether to make one.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("//").map_or(url, |(_, rest)| rest);
    let host = rest
        .split(['/', '?', '#'])
        .next()?
        .rsplit('@')
        .next()?
        .split(':')
        .next()?;
    (!host.is_empty() && host.contains('.')).then(|| host.to_ascii_lowercase())
}

/// Keeps only allowlisted CSS declarations with inert values.
fn filter_inline_style(value: &str) -> String {
    let mut kept: Vec<String> = Vec::new();
    for declaration in value.split(';') {
        let Some((property, raw)) = declaration.split_once(':') else {
            continue;
        };
        let property = property.trim().to_ascii_lowercase();
        let raw = raw.trim();
        if raw.is_empty() || !ALLOWED_STYLE_PROPERTIES.contains(&property.as_str()) {
            continue;
        }
        let lowered = raw.to_ascii_lowercase();
        if CSS_POISON.iter().any(|poison| lowered.contains(poison)) {
            continue;
        }
        // `!important` in a message body has no legitimate purpose once the
        // frame owns the cascade, and it is a common override vector.
        if lowered.contains('!') || raw.contains('<') || raw.contains('>') {
            continue;
        }
        kept.push(format!("{property}: {raw}"));
    }
    kept.join("; ")
}
