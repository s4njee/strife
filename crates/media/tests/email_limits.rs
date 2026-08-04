//! Every parser limit, exercised against synthetic hostile input.
//!
//! Each case answers the same question: can one message consume more than its
//! share? The messages are generated rather than committed as fixtures, because
//! a 64 MB `.eml` does not belong in a repository and the shapes here are
//! defined by their size rather than by their content.

use std::fmt::Write as _;

use strife_media::{AttachmentLimits, EmailParseLimits, parse_email, parse_email_with_parts};

fn limits() -> EmailParseLimits {
    EmailParseLimits::default()
}

/// A minimal well-formed message with a caller-supplied body.
fn message(body: &str) -> String {
    format!(
        "From: ada@example.test\r\nTo: bob@example.test\r\nSubject: Limits\r\n\
         Date: Mon, 1 Jan 2018 00:00:00 +0000\r\nMessage-ID: <limits@example.test>\r\n\r\n{body}"
    )
}

#[test]
fn a_message_over_the_source_limit_is_rejected_before_parsing() {
    let oversized = message(&"x".repeat(2048));
    let error = parse_email(
        oversized.as_bytes(),
        EmailParseLimits {
            max_source_bytes: 256,
            ..limits()
        },
    )
    .expect_err("oversized message was parsed");
    let detail = format!("{error:#}");
    // The message names the limit it hit, so an operator reading a failed row
    // knows which knob to turn rather than guessing.
    assert!(detail.contains("source size limit exceeded"), "{detail}");
    assert!(detail.contains("256"), "{detail}");
}

#[test]
fn a_message_with_too_many_mime_parts_is_rejected() {
    // Each part is trivial; the cost is in the count. This is the shape that a
    // per-part size limit alone would not catch.
    let mut body = String::new();
    for index in 0..60 {
        let _ = write!(
            body,
            "--b\r\nContent-Type: text/plain\r\n\r\npart {index}\r\n"
        );
    }
    body.push_str("--b--\r\n");
    let source = format!(
        "From: ada@example.test\r\nTo: bob@example.test\r\nSubject: Many parts\r\n\
         MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n{body}"
    );

    let error = parse_email(
        source.as_bytes(),
        EmailParseLimits {
            max_parts: 8,
            ..limits()
        },
    )
    .expect_err("a message over the part limit was parsed");
    let detail = format!("{error:#}");
    assert!(detail.contains("MIME part limit exceeded"), "{detail}");
}

#[test]
fn an_enormous_header_value_is_truncated_rather_than_dropped() {
    let source = format!(
        "From: ada@example.test\r\nTo: bob@example.test\r\nSubject: Big header\r\n\
         Received: {}\r\n\r\nBody.",
        "a".repeat(8192)
    );
    let parsed = parse_email(
        source.as_bytes(),
        EmailParseLimits {
            max_header_bytes: 256,
            ..limits()
        },
    )
    .expect("parse");

    let received = parsed
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("received"))
        .expect("the header survived");
    assert!(received.value.len() <= 256);
    // Dropping it entirely would hide that the message had the header at all.
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("truncated")),
        "{:?}",
        parsed.warnings
    );
}

#[test]
fn header_count_is_bounded() {
    let mut headers = String::from("From: ada@example.test\r\nTo: bob@example.test\r\n");
    for index in 0..200 {
        let _ = write!(headers, "X-Custom-{index}: value\r\n");
    }
    let source = format!("{headers}\r\nBody.");
    let parsed = parse_email(
        source.as_bytes(),
        EmailParseLimits {
            max_headers: 10,
            ..limits()
        },
    )
    .expect("parse");
    assert_eq!(parsed.headers.len(), 10);
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("header count limit")),
        "{:?}",
        parsed.warnings
    );
}

#[test]
fn a_body_over_the_limit_is_truncated_and_the_message_still_parses() {
    let parsed = parse_email(
        message(&"word ".repeat(5000)).as_bytes(),
        EmailParseLimits {
            max_body_bytes: 512,
            ..limits()
        },
    )
    .expect("parse");
    assert!(parsed.body_text.len() <= 512);
    // Truncating the body keeps the message searchable; failing it would lose a
    // message whose only sin was being long.
    assert!(!parsed.body_text.is_empty());
}

#[test]
fn stored_warnings_are_capped() {
    let mut headers = String::from("From: ada@example.test\r\n");
    for index in 0..50 {
        let _ = write!(headers, "X-Long-{index}: {}\r\n", "a".repeat(4096));
    }
    let source = format!("{headers}\r\nBody.");
    let parsed = parse_email(
        source.as_bytes(),
        EmailParseLimits {
            max_header_bytes: 64,
            max_warnings: 3,
            ..limits()
        },
    )
    .expect("parse");
    // An unbounded warning list is its own storage problem.
    assert!(parsed.warnings.len() <= 3);
}

#[test]
fn attachment_count_is_bounded() {
    let mut body = String::new();
    for index in 0..20 {
        let _ = write!(
            body,
            "--b\r\nContent-Type: application/octet-stream\r\n\
             Content-Disposition: attachment; filename=\"f{index}.bin\"\r\n\r\ndata\r\n"
        );
    }
    body.push_str("--b--\r\n");
    let source = format!(
        "From: ada@example.test\r\nSubject: Many attachments\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n{body}"
    );
    let parsed = parse_email(
        source.as_bytes(),
        EmailParseLimits {
            max_attachments: 5,
            ..limits()
        },
    )
    .expect("parse");
    assert_eq!(parsed.attachments.len(), 5);
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("attachment count limit")),
        "{:?}",
        parsed.warnings
    );
}

#[test]
fn decoded_attachment_bytes_are_bounded_per_part_and_per_message() {
    let mut body = String::new();
    for index in 0..3 {
        let _ = write!(
            body,
            "--b\r\nContent-Type: application/octet-stream\r\n\
             Content-Disposition: attachment; filename=\"f{index}.bin\"\r\n\r\n{}\r\n",
            "x".repeat(400)
        );
    }
    body.push_str("--b--\r\n");
    let source = format!(
        "From: ada@example.test\r\nSubject: Fat attachments\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n{body}"
    );

    // Per part: nothing fits, so nothing is written.
    let per_part = parse_email_with_parts(
        source.as_bytes(),
        limits(),
        AttachmentLimits {
            max_part_bytes: 100,
            ..AttachmentLimits::default()
        },
    )
    .expect("parse");
    assert!(per_part.parts.is_empty());

    // Per message: the first fits, the rest stop the run.
    let per_message = parse_email_with_parts(
        source.as_bytes(),
        limits(),
        AttachmentLimits {
            max_message_bytes: 500,
            ..AttachmentLimits::default()
        },
    )
    .expect("parse");
    assert_eq!(per_message.parts.len(), 1);
    assert!(
        per_message
            .email
            .warnings
            .iter()
            .any(|warning| warning.contains("total exceeded")),
        "{:?}",
        per_message.email.warnings
    );
}

#[test]
fn deeply_recursive_mime_terminates_and_stays_bounded() {
    // A message nested many levels deep is the classic parser bomb: each level
    // is cheap, and a recursive walker without a depth cap does not return.
    let mut body = String::from("Innermost body.\r\n");
    for depth in (0..40).rev() {
        body = format!(
            "--b{depth}\r\nContent-Type: multipart/mixed; boundary=\"b{}\"\r\n\r\n{body}\r\n--b{depth}--\r\n",
            depth + 1
        );
    }
    let source = format!(
        "From: ada@example.test\r\nSubject: Nested\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"b0\"\r\n\r\n{body}"
    );

    // Either it parses within its limits or it is rejected by one. What it must
    // not do is hang or exhaust memory, which is what this test is really for.
    let outcome = parse_email_with_parts(
        source.as_bytes(),
        EmailParseLimits {
            max_parts: 64,
            ..limits()
        },
        AttachmentLimits::default(),
    );
    match outcome {
        Ok(parsed) => {
            for part in &parsed.parts {
                assert!(
                    part.depth <= AttachmentLimits::default().max_depth,
                    "a part below the depth cap was materialized"
                );
            }
        }
        Err(error) => {
            let detail = format!("{error:#}");
            assert!(detail.contains("limit exceeded"), "{detail}");
        }
    }
}

#[test]
fn a_truncated_message_fails_without_panicking() {
    for source in [
        &b""[..],
        b"From: ada@example.test",
        b"From: ada@example.test\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\n",
    ] {
        // A partially written file must produce an error, not a crash: the
        // watched-folder importer can see one mid-copy.
        let _ = parse_email(source, limits());
    }
}

/// A NUL anywhere in a message must not reach the database.
///
/// NUL is valid UTF-8, so it survives every decode and validation step in the
/// parser and only fails at the `PostgreSQL` insert, which rejects the entire
/// statement with `invalid byte sequence for encoding "UTF8": 0x00`. On the
/// Orion archive this failed roughly one message in 650 — not in the body
/// alone, which is why this covers the header, subject, and filename paths that
/// never pass through body normalization.
#[test]
fn nul_bytes_are_removed_from_every_persisted_field() {
    let raw = "From: \"a\0da\" <ada@example.test>\r\n\
         To: bob@example.test\r\n\
         Subject: quarterly\0 report\r\n\
         X-Odd-Header: value\0with\0nuls\r\n\
         Date: Mon, 1 Jan 2018 00:00:00 +0000\r\n\
         Message-ID: <nul@example.test>\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=b\r\n\r\n\
         --b\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\r\n\
         body text with a \0 in it\r\n\
         --b\r\n\
         Content-Type: application/octet-stream\r\n\
         Content-Disposition: attachment; filename=\"re\0port.bin\"\r\n\r\n\
         payload\r\n\
         --b--\r\n";

    let parsed = parse_email(raw.as_bytes(), limits()).expect("a NUL must not fail the parse");

    // Whatever survives, none of it may carry a NUL into PostgreSQL.
    let mut checked = vec![
        parsed.body_text.clone(),
        parsed.preview_text.clone(),
        parsed.content_hash.clone(),
    ];
    checked.extend(parsed.subject.clone());
    checked.extend(parsed.normalized_subject.clone());
    checked.extend(parsed.body_html.clone());
    checked.extend(parsed.labels.iter().cloned());
    checked.extend(parsed.warnings.iter().cloned());
    for address in &parsed.addresses {
        checked.push(address.address.clone());
        checked.extend(address.display_name.clone());
    }
    for header in &parsed.headers {
        checked.push(header.name.clone());
        checked.push(header.value.clone());
    }
    for attachment in &parsed.attachments {
        checked.push(attachment.part_path.clone());
        checked.push(attachment.media_type.clone());
        checked.extend(attachment.filename.clone());
        checked.extend(attachment.disposition.clone());
    }
    for value in &checked {
        assert!(
            !value.contains('\0'),
            "a NUL survived into a persisted field: {value:?}"
        );
    }

    // The surrounding text is kept; only the NUL is dropped.
    assert!(
        parsed.body_text.contains("body text with a"),
        "{:?}",
        parsed.body_text
    );
    assert_eq!(parsed.subject.as_deref(), Some("quarterly report"));
    assert!(
        parsed.warnings.iter().any(|w| w.contains("NUL")),
        "removal must be reported: {:?}",
        parsed.warnings
    );
}

/// The stored hash must be derivable from the stored text.
#[test]
fn the_content_hash_is_computed_after_nul_removal() {
    let with_nul = "From: ada@example.test\r\nTo: bob@example.test\r\n\
         Subject: same\r\nDate: Mon, 1 Jan 2018 00:00:00 +0000\r\n\
         Message-ID: <a@example.test>\r\n\r\nidentical\0 body\r\n";
    let without_nul = "From: ada@example.test\r\nTo: bob@example.test\r\n\
         Subject: same\r\nDate: Mon, 1 Jan 2018 00:00:00 +0000\r\n\
         Message-ID: <b@example.test>\r\n\r\nidentical body\r\n";

    let dirty = parse_email(with_nul.as_bytes(), limits()).expect("parse with NUL");
    let clean = parse_email(without_nul.as_bytes(), limits()).expect("parse without NUL");

    // Hashing before removal would make these differ, and a message would then
    // carry a hash that cannot be reproduced from what was actually stored.
    assert_eq!(dirty.body_text, clean.body_text);
    assert_eq!(dirty.content_hash, clean.content_hash);
}
