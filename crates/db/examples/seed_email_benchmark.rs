//! Generates a synthetic email corpus for the search benchmark.
//!
//! Produces archive-shaped data — long-tailed body sizes, bounded correspondent
//! cardinality, skewed labels, a decade of sent dates — so measured latency
//! reflects a real archive rather than uniform random text. Every identity is
//! synthetic; this never copies mailbox content.
//!
//! Casts here are bounded by construction; this is a seeding tool, not a
//! library.
//!
//! ```bash
//! cargo run --release -p strife-db --example seed_email_benchmark -- \
//!   --database-url "$DATABASE_URL" --messages 100000
//! ```

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::too_many_lines
)]

use std::time::Instant;

use chrono::{Duration, TimeZone, Utc};
use sqlx::postgres::PgPoolOptions;
use strife_db::{
    EmailAddressInput, EmailAddressRole, EmailAttachmentInput, EmailExtractionStatus,
    EmailProjection, MIGRATOR, ROOT_NODE_ID, UpsertEmailMessage, replace_email_projection,
};
use uuid::Uuid;

const LABELS: [&str; 8] = [
    "Inbox",
    "Work",
    "Personal",
    "Receipts",
    "Travel",
    "Newsletters",
    "Archive",
    "Important",
];

const WORDS: [&str; 24] = [
    "invoice",
    "schedule",
    "reconciliation",
    "quarterly",
    "shipment",
    "renewal",
    "itinerary",
    "proposal",
    "contract",
    "deployment",
    "incident",
    "retrospective",
    "budget",
    "forecast",
    "onboarding",
    "handover",
    "logistics",
    "compliance",
    "settlement",
    "amendment",
    "inspection",
    "warranty",
    "subscription",
    "reimbursement",
];

/// Deterministic PRNG so a benchmark run is reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }
}

/// Long-tailed body length: most messages are short, a few are very large.
fn body_words(rng: &mut Rng) -> usize {
    match rng.below(100) {
        0..=69 => 40 + rng.below(120) as usize,
        70..=94 => 200 + rng.below(800) as usize,
        95..=98 => 1_000 + rng.below(4_000) as usize,
        _ => 5_000 + rng.below(20_000) as usize,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut database_url = None;
    let mut messages = 100_000_u64;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--database-url" => database_url = args.next(),
            "--messages" => {
                messages = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(messages);
            }
            other => anyhow::bail!("unknown argument {other}"),
        }
    }
    let database_url = database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| anyhow::anyhow!("--database-url or DATABASE_URL is required"))?;

    // Refuse to write into anything that is not clearly a benchmark database.
    anyhow::ensure!(
        database_url.contains("benchmark"),
        "refusing to seed a database whose name does not contain 'benchmark'; \
         create a dedicated one rather than polluting the archive"
    );

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await?;
    MIGRATOR.run(&pool).await?;

    // Bounded correspondent cardinality: a few frequent, a long tail.
    let frequent: Vec<String> = (0..200)
        .map(|index| format!("frequent{index}@example.test"))
        .collect();
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    let started = Instant::now();
    let epoch = Utc.with_ymd_and_hms(2015, 1, 1, 0, 0, 0).unwrap();

    for index in 0..messages {
        let node_id = Uuid::new_v4();
        sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
            .bind(node_id)
            .bind(ROOT_NODE_ID)
            .bind(format!("bench-{node_id}.eml"))
            .execute(&pool)
            .await?;

        let subject = format!(
            "{} {} {}",
            WORDS[rng.below(WORDS.len() as u64) as usize],
            WORDS[rng.below(WORDS.len() as u64) as usize],
            index
        );
        let body: String = (0..body_words(&mut rng))
            .map(|_| WORDS[rng.below(WORDS.len() as u64) as usize])
            .collect::<Vec<_>>()
            .join(" ");
        let from = if rng.below(100) < 80 {
            frequent[rng.below(frequent.len() as u64) as usize].clone()
        } else {
            format!("tail{}@example.test", rng.next())
        };
        // Skewed label frequency rather than uniform selection.
        let label_count = usize::from(rng.below(100) < 60) + usize::from(rng.below(100) < 20);
        let mut labels: Vec<String> = (0..label_count)
            .map(|_| LABELS[(rng.below(100) % LABELS.len() as u64) as usize].to_owned())
            .collect();
        labels.sort();
        labels.dedup();
        let attachments: Vec<EmailAttachmentInput<'_>> = if rng.below(100) < 20 {
            vec![EmailAttachmentInput {
                part_path: "2",
                filename: Some("attachment.pdf"),
                media_type: "application/pdf",
                disposition: Some("attachment"),
                content_id: None,
                transfer_encoding: Some("base64"),
                decoded_size: Some(i64::try_from(rng.below(4_000_000)).unwrap_or(0)),
                checksum_sha256: None,
                is_inline: false,
                is_message: false,
                warnings: &[],
            }]
        } else {
            Vec::new()
        };
        let sent_at = epoch + Duration::minutes(i64::try_from(rng.below(5_256_000)).unwrap_or(0));

        replace_email_projection(
            &pool,
            &EmailProjection {
                message: UpsertEmailMessage {
                    node_id,
                    status: EmailExtractionStatus::Completed,
                    parser_name: "benchmark",
                    parser_version: "0.11.5",
                    message_id: None,
                    normalized_message_id: None,
                    in_reply_to: None,
                    reference_ids: &[],
                    subject: Some(&subject),
                    normalized_subject: Some(&subject),
                    sent_at: Some(sent_at),
                    received_at: None,
                    body_text: &body,
                    body_html: None,
                    preview_text: &body[..body.len().min(240)],
                    content_hash: None,
                    provider_thread_id: None,
                    warnings: &[],
                    duration_ms: None,
                },
                addresses: &[
                    EmailAddressInput {
                        role: EmailAddressRole::From,
                        display_name: None,
                        address: &from,
                    },
                    EmailAddressInput {
                        role: EmailAddressRole::To,
                        display_name: None,
                        address: "owner@example.test",
                    },
                ],
                headers: &[],
                labels: &labels,
                attachments: &attachments,
            },
        )
        .await?;

        if index > 0 && index % 5_000 == 0 {
            let rate = index as f64 / started.elapsed().as_secs_f64();
            println!("{index} messages seeded ({rate:.0}/s)");
        }
    }

    let elapsed = started.elapsed();
    println!(
        "seeded {messages} messages in {:.1}s ({:.0}/s)",
        elapsed.as_secs_f64(),
        messages as f64 / elapsed.as_secs_f64()
    );
    println!("record the results in docs/benchmarks/email-search.md");
    Ok(())
}
