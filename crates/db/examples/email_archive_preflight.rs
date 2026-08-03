//! Read-only survey of a real archive before any backfill is authorized.
//!
//! Answers the questions [`backfill.md`](../../../docs/backfill.md) requires an
//! operator to answer *before* starting a campaign: how many files, how big,
//! how confident the MIME detection is, how many look malformed, how many are
//! probably duplicates, and how much database and artifact disk the run will
//! need.
//!
//! Two properties are load-bearing:
//!
//! - **It never writes.** No projection, no job, no campaign, no artifact. It
//!   can be run against production at any time, including mid-backfill.
//! - **It never prints message content.** Subjects, addresses, and bodies stay
//!   out of the report; the archive is surveyed by shape, not by reading it.
//!
//! ```bash
//! cargo run --release -p strife-db --example email_archive_preflight -- \
//!   --path /srv/strife/import/mail
//! ```

// Casts here are bounded by construction and feed a human-readable estimate,
// not a calculation anything depends on. This is a survey tool, not a library.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use strife_media::{EmailParseLimits, looks_like_rfc822, normalize_message_id, parse_email};

/// Bytes read from the head of each file for detection. Enough to cover a
/// header block without reading a gigabyte of attachments to classify a file.
const SNIFF_BYTES: usize = 8192;

#[derive(Default)]
struct Survey {
    files: u64,
    total_bytes: u64,
    sizes: Vec<u64>,
    rfc822_confident: u64,
    /// Looks like a message but the parser rejected it.
    malformed: u64,
    /// Does not look like a message at all.
    not_email: u64,
    unreadable: u64,
    message_ids: HashMap<String, u32>,
    largest: u64,
}

fn percentile(sorted: &[u64], fraction: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn walk(path: &Path, survey: &mut Survey, limits: EmailParseLimits) {
    let Ok(entries) = std::fs::read_dir(path) else {
        survey.unreadable += 1;
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, survey, limits);
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            survey.unreadable += 1;
            continue;
        };
        let size = metadata.len();
        survey.files += 1;
        survey.total_bytes += size;
        survey.sizes.push(size);
        survey.largest = survey.largest.max(size);

        let Ok(bytes) = std::fs::read(&path) else {
            survey.unreadable += 1;
            continue;
        };
        let head = &bytes[..bytes.len().min(SNIFF_BYTES)];
        if !looks_like_rfc822(head) {
            survey.not_email += 1;
            continue;
        }
        survey.rfc822_confident += 1;

        // Parsing is what makes the malformed count trustworthy, and it is also
        // the closest available estimate of how the real run will behave.
        match parse_email(&bytes, limits) {
            Ok(parsed) => {
                if let Some(id) = parsed
                    .normalized_message_id
                    .as_deref()
                    .map(normalize_message_id)
                {
                    *survey.message_ids.entry(id).or_default() += 1;
                }
            }
            Err(_) => survey.malformed += 1,
        }
    }
}

fn main() {
    let mut path: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--path" => path = args.next().map(PathBuf::from),
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(path) = path else {
        eprintln!("--path is required");
        std::process::exit(2);
    };

    let limits = EmailParseLimits::default();
    let mut survey = Survey::default();
    let started = std::time::Instant::now();
    walk(&path, &mut survey, limits);
    survey.sizes.sort_unstable();

    let duplicate_groups = survey
        .message_ids
        .values()
        .filter(|count| **count > 1)
        .count();
    let duplicate_copies: u32 = survey
        .message_ids
        .values()
        .filter(|count| **count > 1)
        .map(|count| count - 1)
        .sum();

    // Projected database use. The multiplier is the ratio observed on the
    // fixture corpus — bodies are stored once as text plus a tsvector — and is
    // an estimate to be checked against the first canary, not a guarantee.
    let projected_database = (survey.total_bytes as f64 * 1.6) as u64;
    // Attachments are stored decoded; base64 shrinks by about a quarter.
    let projected_artifacts = (survey.total_bytes as f64 * 0.75) as u64;

    println!("Email archive preflight (read-only)");
    println!("  path                  {}", path.display());
    println!(
        "  scanned in            {:.1}s",
        started.elapsed().as_secs_f64()
    );
    println!();
    println!("  files                 {}", survey.files);
    println!("  total bytes           {}", human(survey.total_bytes));
    println!(
        "  size p50 / p95 / max  {} / {} / {}",
        human(percentile(&survey.sizes, 0.50)),
        human(percentile(&survey.sizes, 0.95)),
        human(survey.largest)
    );
    println!();
    println!("  confident RFC 5322    {}", survey.rfc822_confident);
    println!("  not email             {}", survey.not_email);
    println!("  malformed             {}", survey.malformed);
    println!("  unreadable            {}", survey.unreadable);
    println!();
    println!("  duplicate groups      {duplicate_groups}");
    println!("  redundant copies      {duplicate_copies}");
    println!();
    println!(
        "  projected database    {} (estimate)",
        human(projected_database)
    );
    println!(
        "  projected artifacts   {} (estimate)",
        human(projected_artifacts)
    );
    println!();
    println!("No message content was read into this report, and nothing was written.");
    println!("Record these numbers in docs/backfill.md before authorizing a campaign.");
}
