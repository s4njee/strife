//! Version reporting and read-only repair scanning.
//!
//! The property that matters most here is what repair *cannot* do: reconciling
//! a count must never be a way to start a backfill nobody authorized.

use chrono::{TimeZone, Utc};
use sqlx::PgPool;
use strife_db::{
    BackfillKind, BackfillState, EmailArtifactState, EmailAttachmentInput, EmailExtractionStatus,
    EmailProjection, JobOrigin, JobResourceClass, JobType, NewBackfillCampaign, ROOT_NODE_ID,
    UpsertEmailAttachmentArtifact, UpsertEmailMessage, create_backfill_campaign, email_repair_scan,
    email_version_report, enqueue_job_with_context, list_backfill_campaigns,
    prepare_backfill_campaign, repair_campaign_counts, replace_email_projection,
    transition_backfill_campaign, upsert_email_attachment_artifact,
};
use uuid::Uuid;

async fn seed(pool: &PgPool, parser_version: &str, with_attachment: bool) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("repair-{node_id}.eml"))
        .execute(pool)
        .await
        .expect("create node");
    let attachments = if with_attachment {
        vec![EmailAttachmentInput {
            part_path: "2",
            filename: Some("a.pdf"),
            media_type: "application/pdf",
            disposition: Some("attachment"),
            content_id: None,
            transfer_encoding: None,
            decoded_size: Some(10),
            checksum_sha256: None,
            is_inline: false,
            is_message: false,
            warnings: &[],
        }]
    } else {
        Vec::new()
    };
    replace_email_projection(
        pool,
        &EmailProjection {
            message: UpsertEmailMessage {
                node_id,
                status: EmailExtractionStatus::Completed,
                parser_name: "mail-parser",
                parser_version,
                message_id: Some(&format!("{node_id}@example.test")),
                normalized_message_id: Some(&format!("{node_id}@example.test")),
                in_reply_to: None,
                reference_ids: &[],
                subject: Some("Repair"),
                normalized_subject: Some("Repair"),
                sent_at: Some(Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap()),
                received_at: None,
                body_text: "Body.",
                body_html: None,
                preview_text: "Body.",
                content_hash: None,
                provider_thread_id: None,
                warnings: &[],
                duration_ms: None,
            },
            addresses: &[],
            headers: &[],
            labels: &[],
            attachments: &attachments,
        },
    )
    .await
    .expect("seed projection");
    node_id
}

#[sqlx::test(migrations = "./migrations")]
async fn version_axes_are_reported_and_counted_independently(pool: PgPool) {
    seed(&pool, "0.11.5", false).await;
    seed(&pool, "0.11.5", false).await;
    let stale = seed(&pool, "0.10.0", true).await;
    upsert_email_attachment_artifact(
        &pool,
        &UpsertEmailAttachmentArtifact {
            node_id: stale,
            part_path: "2",
            state: EmailArtifactState::Ready,
            storage_key: Some(&Uuid::new_v4().to_string()),
            media_type: "application/pdf",
            byte_size: 10,
            checksum_sha256: None,
            depth: 0,
            is_message: false,
            materializer_version: "1",
            warnings: &[],
        },
    )
    .await
    .expect("artifact");

    let report = email_version_report(&pool, "0.11.5", "1")
        .await
        .expect("report");
    assert_eq!(report.parser.len(), 2);
    assert_eq!(report.parser[0].version, "0.11.5");
    assert_eq!(report.parser[0].count, 2);
    // Only the message on the older parser needs reparsing; the attachment's
    // own version is unaffected by a parser change, which is the whole reason
    // these are separate axes.
    assert_eq!(report.messages_needing_reparse, 1);
    assert_eq!(report.attachments_needing_reextraction, 0);
    assert_eq!(report.attachment_materializer[0].version, "1");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_clean_archive_reports_nothing_to_repair(pool: PgPool) {
    seed(&pool, "0.11.5", false).await;
    let report = email_repair_scan(&pool).await.expect("scan");
    assert_eq!(report.missing_projections, 0);
    assert_eq!(report.orphan_artifacts, 0);
    assert_eq!(report.artifact_without_manifest, 0);
    assert_eq!(report.stale_leases, 0);
    assert_eq!(report.campaigns_with_count_drift, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_manifest_entry_without_an_artifact_is_detected(pool: PgPool) {
    // The message declares an attachment but materialization never ran, which
    // is what an interrupted backfill leaves behind.
    seed(&pool, "0.11.5", true).await;
    let report = email_repair_scan(&pool).await.expect("scan");
    assert_eq!(report.manifest_without_artifact, 1);
    assert_eq!(report.artifact_without_manifest, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_artifact_without_a_manifest_entry_is_detected(pool: PgPool) {
    let node_id = seed(&pool, "0.11.5", false).await;
    // An artifact for a part the current manifest no longer lists: what a
    // parser change that alters part numbering would produce.
    upsert_email_attachment_artifact(
        &pool,
        &UpsertEmailAttachmentArtifact {
            node_id,
            part_path: "9.9",
            state: EmailArtifactState::Ready,
            storage_key: Some(&Uuid::new_v4().to_string()),
            media_type: "application/pdf",
            byte_size: 10,
            checksum_sha256: None,
            depth: 0,
            is_message: false,
            materializer_version: "1",
            warnings: &[],
        },
    )
    .await
    .expect("artifact");

    let report = email_repair_scan(&pool).await.expect("scan");
    assert_eq!(report.artifact_without_manifest, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_stale_lease_is_detected(pool: PgPool) {
    let node_id = seed(&pool, "0.11.5", false).await;
    enqueue_job_with_context(
        &pool,
        JobType::EmailExtraction,
        node_id,
        0,
        JobOrigin::Foreground,
        None,
        JobResourceClass::HeavyCpu,
    )
    .await
    .expect("enqueue");
    // A worker that died mid-job leaves exactly this: leased, expired, and
    // never renewed.
    sqlx::query(
        "UPDATE jobs SET state = 'leased', lease_expires_at = now() - interval '1 hour'
         WHERE target_node_id = $1",
    )
    .bind(node_id)
    .execute(&pool)
    .await
    .expect("expire lease");

    let report = email_repair_scan(&pool).await.expect("scan");
    assert_eq!(report.stale_leases, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn repairing_counts_cannot_resume_a_paused_campaign(pool: PgPool) {
    let campaign = create_backfill_campaign(
        &pool,
        &NewBackfillCampaign {
            kind: BackfillKind::Email,
            candidate_definition: serde_json::json!({"version": 1}),
            batch_size: 100,
            max_queued: 500,
            max_running: 1,
            resource_class: JobResourceClass::HeavyCpu,
            foreground_fairness: 20,
            created_by_version: "test".to_owned(),
        },
    )
    .await
    .expect("campaign");
    prepare_backfill_campaign(
        &pool,
        campaign.id,
        0,
        Utc::now() + chrono::Duration::minutes(5),
        None,
    )
    .await
    .expect("prepare")
    .expect("draft campaign");

    let repaired = repair_campaign_counts(&pool, campaign.id)
        .await
        .expect("repair");
    // A repair command that could start a ten-year backfill as a side effect of
    // fixing a count would be a very expensive accident.
    assert_eq!(repaired.state, BackfillState::Paused);

    let listed = list_backfill_campaigns(&pool).await.expect("list");
    assert_eq!(listed[0].state, BackfillState::Paused);
}

#[sqlx::test(migrations = "./migrations")]
async fn campaign_count_drift_is_detected_and_reconciled(pool: PgPool) {
    let campaign = create_backfill_campaign(
        &pool,
        &NewBackfillCampaign {
            kind: BackfillKind::Email,
            candidate_definition: serde_json::json!({"version": 1}),
            batch_size: 100,
            max_queued: 500,
            max_running: 1,
            resource_class: JobResourceClass::HeavyCpu,
            foreground_fairness: 20,
            created_by_version: "test".to_owned(),
        },
    )
    .await
    .expect("campaign");
    prepare_backfill_campaign(
        &pool,
        campaign.id,
        0,
        Utc::now() + chrono::Duration::minutes(5),
        None,
    )
    .await
    .expect("prepare")
    .expect("draft campaign");
    transition_backfill_campaign(&pool, campaign.id, BackfillState::Running, None)
        .await
        .expect("run")
        .expect("paused campaign");

    for _ in 0..3 {
        let node_id = seed(&pool, "0.11.5", false).await;
        enqueue_job_with_context(
            &pool,
            JobType::EmailExtraction,
            node_id,
            20,
            JobOrigin::Backfill,
            Some(campaign.id),
            JobResourceClass::HeavyCpu,
        )
        .await
        .expect("enqueue");
    }
    // Simulate a crash between enqueueing jobs and recording that they were
    // enqueued — the window an interrupted refill leaves open.
    sqlx::query("UPDATE backfill_campaigns SET enqueued_count = 0 WHERE id = $1")
        .bind(campaign.id)
        .execute(&pool)
        .await
        .expect("reset count");

    let before = email_repair_scan(&pool).await.expect("scan");
    assert_eq!(before.campaigns_with_count_drift, 1);

    let repaired = repair_campaign_counts(&pool, campaign.id)
        .await
        .expect("repair");
    assert_eq!(repaired.enqueued_count, 3);
    assert_eq!(repaired.state, BackfillState::Running, "state was changed");

    let after = email_repair_scan(&pool).await.expect("scan");
    assert_eq!(after.campaigns_with_count_drift, 0);
}
