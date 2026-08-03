//! Asserts every committed fixture against its `expected.json` entry.
//!
//! Failures report a semantic difference — a wrong subject, a missing
//! attachment, a leaked resource URL — rather than an opaque snapshot diff.

use std::{fs, path::PathBuf};

use strife_media::{EmailAddressKind, EmailParseLimits, ParsedEmail, parse_email};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/email")
}

fn manifest() -> serde_json::Value {
    let raw = fs::read_to_string(fixture_dir().join("expected.json")).expect("read expected.json");
    serde_json::from_str(&raw).expect("expected.json is valid JSON")
}

fn parse_fixture(name: &str) -> ParsedEmail {
    let bytes = fs::read(fixture_dir().join(name)).expect("read fixture");
    parse_email(&bytes, EmailParseLimits::default())
        .unwrap_or_else(|error| panic!("{name} failed to parse: {error:#}"))
}

fn addresses(parsed: &ParsedEmail, kind: EmailAddressKind) -> Vec<String> {
    parsed
        .addresses
        .iter()
        .filter(|address| address.kind == kind)
        .map(|address| address.address.clone())
        .collect()
}

fn expected_list(entry: &serde_json::Value, key: &str) -> Option<Vec<String>> {
    entry.get(key)?.as_array().map(|items| {
        items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect()
    })
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_fixture_matches_its_expected_normalized_output() {
    let manifest = manifest();
    let fixtures = manifest["fixtures"].as_object().expect("fixtures object");

    for (name, entry) in fixtures {
        let parsed = parse_fixture(name);

        if let Some(subject) = entry.get("subject") {
            let expected = subject.as_str();
            assert_eq!(
                parsed.subject.as_deref(),
                expected,
                "{name}: subject mismatch"
            );
        }
        if entry
            .get("message_id")
            .is_some_and(serde_json::Value::is_null)
        {
            assert!(
                parsed.message_id.is_none(),
                "{name}: expected no Message-ID, got {:?}",
                parsed.message_id
            );
        } else if let Some(expected) = entry.get("message_id").and_then(serde_json::Value::as_str) {
            assert_eq!(
                parsed.normalized_message_id.as_deref(),
                Some(expected),
                "{name}: message id mismatch"
            );
        }
        if let Some(expected) = entry.get("sent_at") {
            if expected.is_null() {
                assert!(
                    parsed.sent_at.is_none(),
                    "{name}: unparseable date must stay unset"
                );
            } else if let Some(text) = expected.as_str() {
                assert_eq!(
                    parsed
                        .sent_at
                        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                    Some(text.to_owned()),
                    "{name}: sent_at mismatch"
                );
            }
        }
        for (key, kind) in [
            ("from", EmailAddressKind::From),
            ("to", EmailAddressKind::To),
            ("cc", EmailAddressKind::Cc),
        ] {
            if let Some(expected) = expected_list(entry, key) {
                assert_eq!(addresses(&parsed, kind), expected, "{name}: {key} mismatch");
            }
        }
        if let Some(expected) = expected_list(entry, "from_display_names") {
            let actual: Vec<String> = parsed
                .addresses
                .iter()
                .filter(|address| address.kind == EmailAddressKind::From)
                .filter_map(|address| address.display_name.clone())
                .collect();
            assert_eq!(actual, expected, "{name}: display name mismatch");
        }
        for needle in expected_list(entry, "body_contains").unwrap_or_default() {
            assert!(
                parsed.body_text.contains(&needle),
                "{name}: body missing {needle:?}\nbody was: {:?}",
                parsed.body_text
            );
        }
        for needle in expected_list(entry, "body_excludes").unwrap_or_default() {
            assert!(
                !parsed.body_text.contains(&needle),
                "{name}: body should not contain {needle:?}\nbody was: {:?}",
                parsed.body_text
            );
        }
        if let Some(expected) = entry
            .get("has_html_alternative")
            .and_then(serde_json::Value::as_bool)
        {
            assert_eq!(
                parsed.body_html.is_some(),
                expected,
                "{name}: html alternative retention mismatch"
            );
        }
        if let Some(expected) = entry
            .get("attachment_count")
            .and_then(serde_json::Value::as_u64)
        {
            assert_eq!(
                u64::try_from(parsed.attachments.len()).expect("attachment count fits"),
                expected,
                "{name}: attachment count mismatch, got {:?}",
                parsed
                    .attachments
                    .iter()
                    .map(|a| (&a.part_path, &a.media_type))
                    .collect::<Vec<_>>()
            );
        }
        if let Some(expected) = expected_list(entry, "labels") {
            assert_eq!(parsed.labels, expected, "{name}: labels mismatch");
        }
        if let Some(expected) = entry.get("provider_thread_id") {
            assert_eq!(
                parsed.provider_thread_id.as_deref(),
                expected.as_str(),
                "{name}: provider thread id mismatch"
            );
        }
        if entry
            .get("expect_warning")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            assert!(!parsed.warnings.is_empty(), "{name}: expected a warning");
            if let Some(needle) = entry
                .get("warning_contains")
                .and_then(serde_json::Value::as_str)
            {
                assert!(
                    parsed
                        .warnings
                        .iter()
                        .any(|warning| warning.to_lowercase().contains(needle)),
                    "{name}: no warning mentions {needle:?}; got {:?}",
                    parsed.warnings
                );
            }
        }
        for (index, expected) in entry
            .get("attachments")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let actual = &parsed.attachments[index];
            if let Some(filename) = expected.get("filename").and_then(serde_json::Value::as_str) {
                assert_eq!(
                    actual.filename.as_deref(),
                    Some(filename),
                    "{name}: attachment filename mismatch"
                );
            }
            if let Some(media_type) = expected
                .get("media_type")
                .and_then(serde_json::Value::as_str)
            {
                assert_eq!(
                    actual.media_type, media_type,
                    "{name}: attachment media type mismatch"
                );
            }
            if let Some(content_id) = expected
                .get("content_id")
                .and_then(serde_json::Value::as_str)
            {
                assert_eq!(
                    actual.content_id.as_deref(),
                    Some(content_id),
                    "{name}: content id mismatch"
                );
            }
            for (key, actual_value) in [
                ("is_inline", actual.is_inline),
                ("is_message", actual.is_message),
            ] {
                if let Some(flag) = expected.get(key).and_then(serde_json::Value::as_bool) {
                    assert_eq!(actual_value, flag, "{name}: attachment {key} mismatch");
                }
            }
        }
    }
}

#[test]
fn duplicate_pairs_group_by_the_documented_reason() {
    let manifest = manifest();
    let fixtures = manifest["fixtures"].as_object().expect("fixtures object");

    for (name, entry) in fixtures {
        let Some(pair) = entry
            .get("duplicate_pair")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let left = parse_fixture(name);
        let right = parse_fixture(pair);
        match entry
            .get("duplicate_reason")
            .and_then(serde_json::Value::as_str)
        {
            Some("message_id") => assert_eq!(
                left.normalized_message_id, right.normalized_message_id,
                "{name} and {pair} must share a normalized Message-ID"
            ),
            Some("content_hash") => {
                assert!(
                    left.normalized_message_id.is_none(),
                    "{name} must have no Message-ID for the content fallback to matter"
                );
                assert_eq!(
                    left.content_hash, right.content_hash,
                    "{name} and {pair} must share a canonical content hash"
                );
            }
            other => panic!("{name}: unknown duplicate reason {other:?}"),
        }
    }
}

#[test]
fn trace_headers_do_not_change_the_canonical_content_hash() {
    // The two Message-ID duplicates differ only by their `Received` header.
    // If tracing leaked into the hash, no real duplicate pair would ever match.
    let left = parse_fixture("duplicate-message-id-a.eml");
    let right = parse_fixture("duplicate-message-id-b.eml");
    assert_ne!(
        left.headers, right.headers,
        "fixtures must actually differ in their trace headers"
    );
    assert_eq!(left.content_hash, right.content_hash);
}

#[test]
fn repeated_headers_keep_their_order_and_original_casing() {
    let parsed = parse_fixture("repeated-recipients.eml");
    let to_headers: Vec<&str> = parsed
        .headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("to"))
        .map(|header| header.value.as_str())
        .collect();
    assert_eq!(to_headers.len(), 2, "repeated To headers collapsed");
    assert!(to_headers[0].contains("bob@example.test"));
    assert!(to_headers[1].contains("cleo@example.test"));
    assert!(
        parsed.headers.iter().any(|header| header.name == "Subject"),
        "original header casing was not preserved"
    );
}

#[test]
fn nested_message_is_distinguished_from_a_binary_attachment() {
    let parsed = parse_fixture("nested-rfc822.eml");
    let nested = parsed
        .attachments
        .iter()
        .find(|attachment| attachment.is_message)
        .expect("nested message/rfc822 part");
    assert!(
        !nested.part_path.is_empty(),
        "nested part must have a MIME path"
    );
    assert!(
        parsed
            .attachments
            .iter()
            .all(|a| a.filename.is_none() || !a.is_message),
        "a nested message must not be treated as a named binary attachment"
    );
}

#[test]
fn non_email_input_is_rejected_rather_than_parsed() {
    let error = parse_email(b"%PDF-1.4\n1 0 obj\n", EmailParseLimits::default())
        .expect_err("a PDF must not parse as email");
    assert!(format!("{error:#}").contains("not an RFC 5322 message"));

    let error = parse_email(&[0xff, 0xd8, 0xff, 0xe0], EmailParseLimits::default())
        .expect_err("JPEG bytes must not parse as email");
    assert!(format!("{error:#}").contains("not an RFC 5322 message"));
}

#[test]
fn source_size_limit_is_enforced_before_parsing() {
    let limits = EmailParseLimits {
        max_source_bytes: 16,
        ..EmailParseLimits::default()
    };
    let bytes = fs::read(fixture_dir().join("plain-text.eml")).expect("read fixture");
    let error = parse_email(&bytes, limits).expect_err("oversized input must be refused");
    let text = format!("{error:#}");
    assert!(text.contains("source size limit exceeded"), "got {text}");
}

#[test]
fn body_limit_truncates_and_warns_rather_than_failing() {
    let limits = EmailParseLimits {
        max_body_bytes: 8,
        ..EmailParseLimits::default()
    };
    let bytes = fs::read(fixture_dir().join("plain-text.eml")).expect("read fixture");
    let parsed = parse_email(&bytes, limits).expect("truncation must not fail the message");
    assert!(parsed.body_text.len() <= 8);
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("truncated")),
        "truncation must be recorded, got {:?}",
        parsed.warnings
    );
}
