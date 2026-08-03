//! Minimal HTML-to-text conversion for archived mail.
//!
//! This is deliberately not a browser. It never resolves a URL, loads a
//! resource, or executes anything: it walks the markup once and emits the text
//! a reader would see, preserving enough paragraph, list, and cell structure
//! for search snippets to make sense. Rendering safety for the reader UI is a
//! separate concern handled by the sanitizer in Story 20.4.

/// Elements whose entire contents are dropped rather than flattened to text.
///
/// `script` and `style` are executable or presentational. `head`, `title`, and
/// `noscript` are not body text. Dropping them here also means tracking markup
/// hidden inside a `<style>` block never reaches the search index.
const DROPPED_ELEMENTS: [&str; 5] = ["script", "style", "head", "title", "noscript"];

/// Elements that end a visual block, so their close emits a line break.
const BLOCK_ELEMENTS: [&str; 17] = [
    "p",
    "div",
    "li",
    "tr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "pre",
    "ul",
    "ol",
    "table",
    "section",
    "article",
];

/// Converts an HTML body into plain text.
///
/// Returns the text plus any warnings worth persisting alongside the message.
#[must_use]
pub fn html_to_text(html: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(html.len() / 2);
    let mut warnings = Vec::new();
    let bytes = html.as_bytes();
    let mut index = 0;
    let mut dropping: Option<String> = None;
    let mut saw_hidden = false;

    while index < bytes.len() {
        if bytes[index] == b'<' {
            // Comments can wrap arbitrary markup, including tracking pixels.
            if html[index..].starts_with("<!--") {
                match html[index..].find("-->") {
                    Some(end) => index += end + 3,
                    None => break,
                }
                continue;
            }
            let Some(close) = html[index..].find('>') else {
                // Unterminated tag: treat the remainder as text rather than
                // silently discarding a truncated message body.
                warnings.push("html body ends inside an unclosed tag".to_owned());
                push_text(&mut out, &html[index..]);
                break;
            };
            let raw = &html[index + 1..index + close];
            index += close + 1;
            let is_end = raw.starts_with('/');
            let name = element_name(raw);

            if let Some(open) = dropping.as_deref() {
                if is_end && name == open {
                    dropping = None;
                }
                continue;
            }
            if !is_end && DROPPED_ELEMENTS.contains(&name.as_str()) {
                // A self-closing dropped element has nothing to skip to.
                if !raw.ends_with('/') {
                    dropping = Some(name);
                }
                continue;
            }
            if !is_end && is_hidden(raw) {
                saw_hidden = true;
                dropping = Some(name);
                continue;
            }
            if name == "br" {
                out.push('\n');
            } else if name == "li" && !is_end {
                push_break(&mut out);
            } else if (name == "td" || name == "th") && is_end {
                out.push('\t');
            } else if BLOCK_ELEMENTS.contains(&name.as_str()) {
                push_break(&mut out);
            }
            continue;
        }

        let next = html[index..].find('<').map_or(bytes.len(), |at| index + at);
        // Text inside a dropped element is skipped, not emitted. Without this
        // the contents of `<script>`, `<style>`, and hidden tracking blocks
        // would still reach the indexed body even though their tags were
        // recognized and discarded.
        if dropping.is_none() {
            push_text(&mut out, &html[index..next]);
        }
        index = next;
    }

    if saw_hidden {
        warnings.push("hidden html content was excluded from the indexed body".to_owned());
    }
    (collapse(&out), warnings)
}

fn element_name(raw: &str) -> String {
    raw.trim_start_matches('/')
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Detects the usual ways archived mail hides tracking content.
fn is_hidden(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    lower.contains("display:none")
        || lower.contains("display: none")
        || lower.contains("visibility:hidden")
        || lower.contains("visibility: hidden")
        || lower.contains(" hidden")
}

fn push_break(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn push_text(out: &mut String, raw: &str) {
    for chunk in raw.split('&') {
        if out.is_empty() && chunk.is_empty() {
            continue;
        }
        match chunk.find(';') {
            Some(end) if end <= 8 => {
                out.push_str(&decode_entity(&chunk[..end]));
                out.push_str(&chunk[end + 1..]);
            }
            _ => out.push_str(chunk),
        }
    }
}

fn decode_entity(name: &str) -> String {
    match name {
        "nbsp" => " ".to_owned(),
        "amp" => "&".to_owned(),
        "lt" => "<".to_owned(),
        "gt" => ">".to_owned(),
        "quot" => "\"".to_owned(),
        "apos" | "#39" => "'".to_owned(),
        "mdash" => "—".to_owned(),
        "ndash" => "–".to_owned(),
        other => other
            .strip_prefix('#')
            .and_then(|digits| {
                let value = digits.strip_prefix('x').map_or_else(
                    || digits.parse::<u32>().ok(),
                    |hex| u32::from_str_radix(hex, 16).ok(),
                )?;
                char::from_u32(value).map(|character| character.to_string())
            })
            .unwrap_or_else(|| format!("&{other};")),
    }
}

/// Normalizes whitespace without destroying paragraph boundaries.
fn collapse(raw: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in raw.replace('\r', "").split('\n') {
        let squeezed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if squeezed.is_empty() {
            if lines.last().is_some_and(|last| !last.is_empty()) {
                lines.push(String::new());
            }
        } else {
            lines.push(squeezed);
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::html_to_text;

    #[test]
    fn preserves_block_and_list_boundaries() {
        let (text, _) = html_to_text(
            "<html><body><p>First paragraph.</p><ul><li>Alpha</li><li>Beta</li></ul>\
             <p>Second&nbsp;paragraph.</p></body></html>",
        );
        assert_eq!(text, "First paragraph.\nAlpha\nBeta\nSecond paragraph.");
    }

    #[test]
    fn drops_scripts_styles_and_comments_without_fetching() {
        let (text, _) = html_to_text(
            "<p>Visible.</p><script>alert('x')</script><style>.a{background:url(http://t.test/p.gif)}</style>\
             <!-- <img src=\"http://tracker.test/pixel.gif\"> --><p>Also visible.</p>",
        );
        assert_eq!(text, "Visible.\nAlso visible.");
        assert!(!text.contains("tracker.test"));
        assert!(!text.contains("alert"));
    }

    #[test]
    fn excludes_hidden_tracking_content_and_warns() {
        let (text, warnings) =
            html_to_text("<p>Shown.</p><div style=\"display:none\">Hidden preheader</div>");
        assert_eq!(text, "Shown.");
        assert!(warnings.iter().any(|warning| warning.contains("hidden")));
    }

    #[test]
    fn keeps_table_cells_separable() {
        let (text, _) = html_to_text(
            "<table><tr><td>Item</td><td>Value</td></tr><tr><td>A</td><td>1</td></tr></table>",
        );
        assert_eq!(text, "Item Value\nA 1");
    }

    #[test]
    fn resource_urls_never_reach_the_indexed_text() {
        let (text, _) = html_to_text(
            "<p>Logo</p><img src=\"http://cdn.test/logo.png\"><a href=\"http://link.test/x\">Click</a>",
        );
        assert!(
            !text.contains("cdn.test"),
            "image URL leaked into body text"
        );
        assert!(text.contains("Click"), "link text must survive");
    }

    #[test]
    fn unterminated_tag_warns_instead_of_truncating() {
        let (text, warnings) = html_to_text("<p>Kept.</p><div style=\"");
        assert!(text.contains("Kept."));
        assert!(warnings.iter().any(|warning| warning.contains("unclosed")));
    }
}
