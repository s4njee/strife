//! Guards the committed email fixture corpus.
//!
//! The parser adapter lands in Story 18.1 and will assert each fixture against
//! its `expected.json` entry. Until then these checks keep the corpus itself
//! trustworthy: every fixture is described, every description has a file, the
//! wire format is valid, and no personal data is committed.

use std::{collections::BTreeSet, fs, path::PathBuf};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/email")
}

fn manifest() -> serde_json::Value {
    let path = fixture_dir().join("expected.json");
    let raw = fs::read_to_string(&path).expect("read expected.json");
    serde_json::from_str(&raw).expect("expected.json is valid JSON")
}

fn fixture_names() -> BTreeSet<String> {
    fs::read_dir(fixture_dir())
        .expect("read fixture directory")
        .filter_map(|entry| {
            let path = entry.expect("fixture dir entry").path();
            (path.extension()? == "eml")
                .then(|| path.file_name()?.to_str().map(str::to_owned))
                .flatten()
        })
        .collect()
}

#[test]
fn every_fixture_is_described_and_every_description_has_a_fixture() {
    let manifest = manifest();
    let described: BTreeSet<String> = manifest["fixtures"]
        .as_object()
        .expect("fixtures object")
        .keys()
        .cloned()
        .collect();
    let present = fixture_names();

    let undocumented: Vec<_> = present.difference(&described).collect();
    assert!(
        undocumented.is_empty(),
        "fixtures without an expected.json entry: {undocumented:?}"
    );
    let missing: Vec<_> = described.difference(&present).collect();
    assert!(
        missing.is_empty(),
        "expected.json describes missing fixtures: {missing:?}"
    );
}

#[test]
fn corpus_covers_the_required_mime_edge_cases() {
    let manifest = manifest();
    let covered: BTreeSet<String> = manifest["fixtures"]
        .as_object()
        .expect("fixtures object")
        .values()
        .flat_map(|entry| {
            entry["covers"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect();
    let joined = covered.into_iter().collect::<Vec<_>>().join(" | ");

    // Story 17.4 names these explicitly; losing one silently would let a
    // parser regression through unnoticed.
    for required in [
        "text/plain",
        "HTML-only",
        "multipart/alternative",
        "multipart/mixed",
        "cid:",
        "message/rfc822",
        "quoted-printable",
        "base64",
        "UTF-8",
        "ISO-8859-1",
        "RFC 2047",
        "folded header",
        "repeated To",
        "absent Message-ID",
        "unparseable Date",
        "MIME boundary",
        "X-Gmail-Labels",
        "without Gmail headers",
        "duplicate pair by Message-ID",
        "duplicate pair by canonical content",
    ] {
        assert!(
            joined.contains(required),
            "no fixture covers {required:?}; corpus coverage regressed"
        );
    }
}

#[test]
fn fixtures_use_crlf_line_endings_and_separate_headers_from_body() {
    for name in fixture_names() {
        let bytes = fs::read(fixture_dir().join(&name)).expect("read fixture");
        assert!(
            bytes.windows(2).any(|pair| pair == b"\r\n"),
            "{name} is not in RFC 5322 CRLF wire format"
        );
        assert!(
            bytes.windows(4).any(|quad| quad == b"\r\n\r\n"),
            "{name} has no header/body separator"
        );
        // A bare LF not preceded by CR would make part boundaries ambiguous.
        let bare_lf = bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| *byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r'));
        assert!(!bare_lf, "{name} contains a bare LF");
    }
}

#[test]
fn fixtures_contain_only_synthetic_identities() {
    for name in fixture_names() {
        let bytes = fs::read(fixture_dir().join(&name)).expect("read fixture");
        // Fixtures are latin-1-safe by construction; lossy is fine for scanning.
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            let lower = line.to_ascii_lowercase();
            if let Some(at) = lower.find('@') {
                let domain: String = lower[at + 1..]
                    .chars()
                    .take_while(|character| {
                        character.is_ascii_alphanumeric() || *character == '.' || *character == '-'
                    })
                    .collect();
                assert!(
                    domain.ends_with("example.test"),
                    "{name} references non-synthetic domain {domain:?}"
                );
            }
        }
        for forbidden in [
            "dkim-signature",
            "authentication-results",
            "x-google-dkim",
            "received-spf",
        ] {
            assert!(
                !text.to_ascii_lowercase().contains(forbidden),
                "{name} commits a real {forbidden} header"
            );
        }
    }
}

#[test]
fn duplicate_pairs_reference_each_other() {
    let manifest = manifest();
    let fixtures = manifest["fixtures"].as_object().expect("fixtures object");
    for (name, entry) in fixtures {
        let Some(pair) = entry["duplicate_pair"].as_str() else {
            continue;
        };
        let other = fixtures
            .get(pair)
            .unwrap_or_else(|| panic!("{name} points at unknown pair {pair}"));
        assert_eq!(
            other["duplicate_pair"].as_str(),
            Some(name.as_str()),
            "{name} and {pair} do not reference each other"
        );
        assert_eq!(
            other["duplicate_reason"], entry["duplicate_reason"],
            "{name} and {pair} disagree on why they are duplicates"
        );
    }
}
