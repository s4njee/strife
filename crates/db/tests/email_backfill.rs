use chrono::{Duration, Utc};
use sqlx::PgPool;
use strife_db::{
    BackfillCampaignRecord, BackfillKind, BackfillState, EmailExtractionStatus,
    EmailReprocessScope, JobOrigin, JobResourceClass, JobType, NewBackfillCampaign, ROOT_NODE_ID,
    UpsertEmailMessage, create_backfill_campaign, create_file_object, email_preflight_report,
    enqueue_email_backfill_batch, enqueue_email_reprocessing, enqueue_job_with_context,
    finalize_file_object, get_backfill_campaign, prepare_backfill_campaign,
    replace_email_projection, transition_backfill_campaign,
};
use uuid::Uuid;

const PARSER: &str = "0.11.5";

async fn seed_file(pool: &PgPool, name: &str, bytes: i64) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("{name}-{node_id}.eml"))
        .execute(pool)
        .await
        .expect("create node");
    let file = create_file_object(pool, Uuid::new_v4(), bytes, Some("message/rfc822"), None)
        .await
        .expect("create file object");
    finalize_file_object(pool, file.id, node_id)
        .await
        .expect("finalize file object");
    node_id
}

async fn seed_projection(
    pool: &PgPool,
    node_id: Uuid,
    version: &str,
    status: EmailExtractionStatus,
) {
    let labels: Vec<String> = Vec::new();
    replace_email_projection(
        pool,
        &strife_db::EmailProjection {
            message: UpsertEmailMessage {
                node_id,
                status,
                parser_name: "mail-parser",
                parser_version: version,
                message_id: None,
                normalized_message_id: None,
                in_reply_to: None,
                reference_ids: &[],
                subject: Some("seeded"),
                normalized_subject: Some("seeded"),
                sent_at: None,
                received_at: None,
                body_text: "seeded",
                body_html: None,
                preview_text: "seeded",
                content_hash: None,
                provider_thread_id: None,
                warnings: &[],
                duration_ms: None,
            },
            addresses: &[],
            headers: &[],
            labels: &labels,
            attachments: &[],
        },
    )
    .await
    .expect("seed projection");
}

fn request() -> NewBackfillCampaign {
    NewBackfillCampaign {
        kind: BackfillKind::Email,
        candidate_definition: serde_json::json!({"version": 1}),
        batch_size: 100,
        max_queued: 500,
        max_running: 1,
        resource_class: JobResourceClass::HeavyCpu,
        foreground_fairness: 20,
        created_by_version: "test".to_owned(),
    }
}

async fn running_campaign(pool: &PgPool) -> BackfillCampaignRecord {
    let campaign = create_backfill_campaign(pool, &request())
        .await
        .expect("create campaign");
    prepare_backfill_campaign(
        pool,
        campaign.id,
        0,
        Utc::now() + Duration::minutes(5),
        None,
    )
    .await
    .expect("prepare")
    .expect("draft campaign");
    transition_backfill_campaign(pool, campaign.id, BackfillState::Running, None)
        .await
        .expect("resume")
        .expect("paused campaign")
}

/// Drops queued email work so the next scope assertion starts from a clean
/// queue; an active job legitimately suppresses a node as a candidate.
async fn clear_email_jobs(pool: &PgPool) {
    sqlx::query("DELETE FROM jobs WHERE job_type = 'email_extraction'")
        .execute(pool)
        .await
        .expect("clear email jobs");
}

async fn campaign_targets(pool: &PgPool, campaign_id: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar::<_, Uuid>("SELECT target_node_id FROM jobs WHERE campaign_id = $1")
        .bind(campaign_id)
        .fetch_all(pool)
        .await
        .expect("list targets")
}

#[sqlx::test(migrations = "./migrations")]
async fn finalizing_an_upload_enqueues_a_foreground_email_job(pool: PgPool) {
    // Exercises the real finalization path, not `finalize_file_object`: the
    // best-effort enqueue lives inside `finalize_upload`.
    let session = strife_db::create_session(
        &pool,
        strife_db::CreateUploadSession {
            target_folder_id: ROOT_NODE_ID,
            display_name: "arrived.eml",
            expected_byte_size: None,
            staging_key: Uuid::new_v4(),
            source_created_at: None,
            source_modified_at: None,
            expires_at: Utc::now() + Duration::hours(1),
        },
    )
    .await
    .expect("create upload session");
    let node = strife_db::finalize_upload(
        &pool,
        session.id,
        Uuid::new_v4(),
        64,
        "message/rfc822",
        &"0".repeat(64),
    )
    .await
    .expect("finalize upload");

    let job = sqlx::query_as::<_, (JobOrigin, Option<Uuid>)>(
        r"
        SELECT origin, campaign_id FROM jobs
        WHERE target_node_id = $1 AND job_type = 'email_extraction'
        ",
    )
    .bind(node.id)
    .fetch_one(&pool)
    .await
    .expect("finalization must enqueue an email job");
    assert_eq!(job.0, JobOrigin::Foreground);
    assert_eq!(job.1, None, "foreground work must not carry a campaign");

    // OCR is enqueued by the same path and must be unaffected.
    let ocr = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM jobs WHERE target_node_id = $1 AND job_type = 'ocr'",
    )
    .bind(node.id)
    .fetch_one(&pool)
    .await
    .expect("count ocr jobs");
    assert_eq!(ocr, 1, "the email enqueue displaced the OCR enqueue");
}

#[sqlx::test(migrations = "./migrations")]
async fn unprepared_campaign_never_enumerates(pool: PgPool) {
    seed_file(&pool, "inert", 1024).await;
    let draft = create_backfill_campaign(&pool, &request())
        .await
        .expect("create draft");
    let (enqueued, exhausted) = enqueue_email_backfill_batch(&pool, &draft, Some(PARSER), 100)
        .await
        .expect("refill draft");
    assert_eq!(enqueued, 0);
    assert!(!exhausted);
    assert!(campaign_targets(&pool, draft.id).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn refill_is_bounded_and_resumes_from_the_cursor(pool: PgPool) {
    let campaign = running_campaign(&pool).await;
    for index in 0..5 {
        seed_file(&pool, &format!("batch-{index}"), 1024).await;
    }

    let (first, exhausted) = enqueue_email_backfill_batch(&pool, &campaign, Some(PARSER), 2)
        .await
        .expect("first batch");
    assert_eq!(first, 2);
    assert!(!exhausted);

    let after = get_backfill_campaign(&pool, campaign.id)
        .await
        .expect("reload")
        .expect("exists");
    assert_eq!(after.enqueued_count, 2);
    assert!(after.cursor_node_id.is_some());

    let (second, _) = enqueue_email_backfill_batch(&pool, &after, Some(PARSER), 2)
        .await
        .expect("second batch");
    assert_eq!(second, 2);

    let mut targets = campaign_targets(&pool, campaign.id).await;
    targets.sort();
    targets.dedup();
    assert_eq!(targets.len(), 4, "a candidate was enqueued twice");
}

#[sqlx::test(migrations = "./migrations")]
async fn campaign_jobs_are_backfill_origin_and_heavy(pool: PgPool) {
    let campaign = running_campaign(&pool).await;
    seed_file(&pool, "classified", 1024).await;

    let (enqueued, _) = enqueue_email_backfill_batch(&pool, &campaign, Some(PARSER), 10)
        .await
        .expect("refill");
    assert_eq!(enqueued, 1);
    let row = sqlx::query_as::<_, (JobType, JobOrigin, JobResourceClass, i32)>(
        "SELECT job_type, origin, resource_class, priority FROM jobs WHERE campaign_id = $1",
    )
    .bind(campaign.id)
    .fetch_one(&pool)
    .await
    .expect("load job");
    assert_eq!(row.0, JobType::EmailExtraction);
    assert_eq!(row.1, JobOrigin::Backfill);
    assert_eq!(row.2, JobResourceClass::HeavyCpu);
    assert_eq!(row.3, -100);
}

#[sqlx::test(migrations = "./migrations")]
async fn nodes_with_active_jobs_are_excluded_before_the_batch_limit(pool: PgPool) {
    let campaign = running_campaign(&pool).await;
    let busy = seed_file(&pool, "busy", 1024).await;
    let free = seed_file(&pool, "free", 1024).await;
    // Foreground work already owns `busy`; the campaign must skip it and still
    // reach `free` rather than reporting a batch that is silently one short.
    enqueue_job_with_context(
        &pool,
        JobType::EmailExtraction,
        busy,
        100,
        JobOrigin::Foreground,
        None,
        JobResourceClass::HeavyCpu,
    )
    .await
    .expect("enqueue competing foreground job");

    let (enqueued, _) = enqueue_email_backfill_batch(&pool, &campaign, Some(PARSER), 10)
        .await
        .expect("refill");
    assert_eq!(enqueued, 1);
    assert_eq!(campaign_targets(&pool, campaign.id).await, vec![free]);
    assert!(
        !campaign_targets(&pool, campaign.id).await.contains(&busy),
        "a node with active foreground work was double-queued"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_second_heavy_campaign_cannot_run_alongside_the_first(pool: PgPool) {
    let _email = running_campaign(&pool).await;
    let ocr = create_backfill_campaign(
        &pool,
        &NewBackfillCampaign {
            kind: BackfillKind::Ocr,
            ..request()
        },
    )
    .await
    .expect("create ocr campaign");
    prepare_backfill_campaign(&pool, ocr.id, 0, Utc::now() + Duration::minutes(5), None)
        .await
        .expect("prepare")
        .expect("draft");

    // Email, OCR, and attachment backfills share one heavy admission permit, so
    // a second heavy campaign must be refused rather than left to compete.
    let refused = transition_backfill_campaign(&pool, ocr.id, BackfillState::Running, None)
        .await
        .expect("transition query");
    assert!(
        refused.is_none(),
        "a second heavy campaign was allowed to run concurrently"
    );
    let reloaded = get_backfill_campaign(&pool, ocr.id)
        .await
        .expect("reload")
        .expect("exists");
    assert_eq!(reloaded.state, BackfillState::Paused);
}

#[sqlx::test(migrations = "./migrations")]
async fn pausing_stops_refills_and_resuming_continues_from_the_cursor(pool: PgPool) {
    let campaign = running_campaign(&pool).await;
    for index in 0..4 {
        seed_file(&pool, &format!("pause-{index}"), 1024).await;
    }

    let (first, _) = enqueue_email_backfill_batch(&pool, &campaign, Some(PARSER), 2)
        .await
        .expect("first batch");
    assert_eq!(first, 2);

    let paused = transition_backfill_campaign(&pool, campaign.id, BackfillState::Paused, None)
        .await
        .expect("pause")
        .expect("running campaign");
    assert_eq!(paused.state, BackfillState::Paused);

    // A paused campaign records no further work even if a refill is attempted.
    let (during_pause, _) = enqueue_email_backfill_batch(&pool, &paused, Some(PARSER), 2)
        .await
        .expect("refill while paused");
    let after_pause = get_backfill_campaign(&pool, campaign.id)
        .await
        .expect("reload")
        .expect("exists");
    assert_eq!(
        after_pause.enqueued_count, 2,
        "a paused campaign advanced its counters (enqueued {during_pause})"
    );

    let resumed = transition_backfill_campaign(&pool, campaign.id, BackfillState::Running, None)
        .await
        .expect("resume")
        .expect("paused campaign");
    let (second, _) = enqueue_email_backfill_batch(&pool, &resumed, Some(PARSER), 2)
        .await
        .expect("second batch");
    assert_eq!(second, 2, "resume did not continue from the cursor");
    let mut targets = campaign_targets(&pool, campaign.id).await;
    targets.sort();
    targets.dedup();
    assert_eq!(targets.len(), 4, "resume rescanned completed nodes");
}

#[sqlx::test(migrations = "./migrations")]
async fn every_reprocess_scope_selects_the_right_nodes(pool: PgPool) {
    let failed = seed_file(&pool, "failed", 1024).await;
    let stale = seed_file(&pool, "stale", 1024).await;
    let current = seed_file(&pool, "current", 1024).await;
    let missing = seed_file(&pool, "missing", 1024).await;
    seed_projection(&pool, failed, PARSER, EmailExtractionStatus::Failed).await;
    seed_projection(&pool, stale, "0.0.1-old", EmailExtractionStatus::Completed).await;
    seed_projection(&pool, current, PARSER, EmailExtractionStatus::Completed).await;

    let count = enqueue_email_reprocessing(&pool, &EmailReprocessScope::Failed, 100)
        .await
        .expect("failed scope");
    assert_eq!(count, 1);

    clear_email_jobs(&pool).await;
    let count = enqueue_email_reprocessing(
        &pool,
        &EmailReprocessScope::VersionMismatch(PARSER.to_owned()),
        100,
    )
    .await
    .expect("version scope");
    assert_eq!(count, 1, "only the stale-parser node is a candidate");

    clear_email_jobs(&pool).await;
    let count = enqueue_email_reprocessing(&pool, &EmailReprocessScope::Missing, 100)
        .await
        .expect("missing scope");
    assert_eq!(count, 1, "only the projection-less node is a candidate");

    clear_email_jobs(&pool).await;
    let count = enqueue_email_reprocessing(&pool, &EmailReprocessScope::Node(missing), 100)
        .await
        .expect("node scope");
    assert_eq!(count, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_duplicate_reprocess_request_is_a_no_op(pool: PgPool) {
    let node_id = seed_file(&pool, "duplicate", 1024).await;

    let first = enqueue_email_reprocessing(&pool, &EmailReprocessScope::Node(node_id), 100)
        .await
        .expect("first request");
    assert_eq!(first, 1);
    let second = enqueue_email_reprocessing(&pool, &EmailReprocessScope::Node(node_id), 100)
        .await
        .expect("second request");
    assert_eq!(second, 0, "a repeated request must enqueue nothing");
}

#[sqlx::test(migrations = "./migrations")]
async fn reprocessing_is_bounded_by_its_batch_limit(pool: PgPool) {
    for index in 0..5 {
        let node_id = seed_file(&pool, &format!("bounded-{index}"), 1024).await;
        seed_projection(&pool, node_id, PARSER, EmailExtractionStatus::Failed).await;
    }

    let count = enqueue_email_reprocessing(&pool, &EmailReprocessScope::Failed, 2)
        .await
        .expect("bounded reprocess");
    assert_eq!(count, 2, "the batch limit was not applied");
}

#[sqlx::test(migrations = "./migrations")]
async fn preflight_reports_candidates_without_enqueueing(pool: PgPool) {
    seed_file(&pool, "preflight", 10_000).await;
    seed_file(&pool, "preflight", 20_000).await;

    let report = email_preflight_report(&pool, Utc::now() + Duration::minutes(5), Some(PARSER))
        .await
        .expect("preflight");
    assert_eq!(report.candidates, 2);
    assert_eq!(report.total_candidate_bytes, 30_000);
    assert!(report.p50_bytes <= report.max_bytes);

    let jobs = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM jobs WHERE job_type = 'email_extraction'",
    )
    .fetch_one(&pool)
    .await
    .expect("count jobs");
    assert_eq!(jobs, 0, "preflight enqueued work");
}

#[sqlx::test(migrations = "./migrations")]
async fn files_created_after_the_snapshot_stay_foreground_work(pool: PgPool) {
    let campaign = running_campaign(&pool).await;
    let late = seed_file(&pool, "late", 1024).await;
    sqlx::query("UPDATE nodes SET created_at = $2 WHERE id = $1")
        .bind(late)
        .bind(Utc::now() + Duration::hours(1))
        .execute(&pool)
        .await
        .expect("push past snapshot");

    let (enqueued, _) = enqueue_email_backfill_batch(&pool, &campaign, Some(PARSER), 10)
        .await
        .expect("refill");
    assert_eq!(enqueued, 0, "campaign enlarged itself past its snapshot");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_trashed_node_is_not_a_backfill_candidate(pool: PgPool) {
    let campaign = running_campaign(&pool).await;
    let node_id = seed_file(&pool, "trashed", 1024).await;
    strife_db::trash_node(&pool, node_id).await.expect("trash");

    let (enqueued, _) = enqueue_email_backfill_batch(&pool, &campaign, Some(PARSER), 10)
        .await
        .expect("refill");
    assert_eq!(enqueued, 0, "a trashed node was queued for backfill");

    // The foreground enqueue helper is also used by the campaign path, so an
    // unrelated regression there would show up as an unexpected job here.
    let stray = enqueue_job_with_context(
        &pool,
        JobType::EmailExtraction,
        node_id,
        0,
        JobOrigin::Foreground,
        None,
        JobResourceClass::HeavyCpu,
    )
    .await
    .expect("manual enqueue is still permitted");
    assert!(stray.is_some(), "trash does not block explicit enqueueing");
}
