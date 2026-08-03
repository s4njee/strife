use chrono::{Duration, Utc};
use sqlx::PgPool;
use strife_db::{
    BackfillCampaignRecord, BackfillKind, BackfillState, JobOrigin, JobResourceClass, JobType,
    NewBackfillCampaign, ROOT_NODE_ID, create_backfill_campaign, enqueue_job_with_context,
    enqueue_ocr_backfill_batch, get_backfill_campaign, ocr_preflight_report,
    prepare_backfill_campaign, transition_backfill_campaign,
};
use uuid::Uuid;

const ENGINE: &str = "tesseract-5.5.0-test";

fn mimes() -> Vec<String> {
    strife_media::supported_ocr_mimes()
        .iter()
        .map(|mime| (*mime).to_owned())
        .collect()
}

/// Creates an active, finalized file node with extracted MIME metadata.
///
/// Candidate selection requires `node_metadata`, so a file whose metadata job
/// has not run is deliberately not seeded by this helper.
async fn seed_candidate(pool: &PgPool, tag: &str, mime: &str, bytes: i64) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("{tag}-{node_id}"))
        .execute(pool)
        .await
        .expect("create candidate node");
    sqlx::query(
        r"
        INSERT INTO file_objects (id, node_id, storage_key, byte_size, checksum_sha256,
                                  upload_state)
        VALUES ($1, $2, $3, $4, $5, 'finalized')
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .bind(format!("originals/{node_id}"))
    .bind(bytes)
    .bind("0".repeat(64))
    .execute(pool)
    .await
    .expect("create finalized file object");
    sqlx::query("INSERT INTO node_metadata (node_id, detected_mime, media_kind) VALUES ($1, $2, 'document')")
        .bind(node_id)
        .bind(mime)
        .execute(pool)
        .await
        .expect("create node metadata");
    node_id
}

fn request(batch_size: i32) -> NewBackfillCampaign {
    NewBackfillCampaign {
        kind: BackfillKind::Ocr,
        candidate_definition: serde_json::json!({"version": 1, "engine": ENGINE}),
        batch_size,
        max_queued: 500,
        max_running: 1,
        resource_class: JobResourceClass::HeavyCpu,
        foreground_fairness: 20,
        created_by_version: "test".to_owned(),
    }
}

async fn running_campaign(pool: &PgPool, batch_size: i32) -> BackfillCampaignRecord {
    let campaign = create_backfill_campaign(pool, &request(batch_size))
        .await
        .expect("create campaign");
    let prepared = prepare_backfill_campaign(
        pool,
        campaign.id,
        0,
        Utc::now() + Duration::minutes(5),
        None,
    )
    .await
    .expect("prepare campaign")
    .expect("draft campaign");
    assert_eq!(prepared.state, BackfillState::Paused);
    transition_backfill_campaign(pool, campaign.id, BackfillState::Running, None)
        .await
        .expect("resume campaign")
        .expect("paused campaign")
}

async fn campaign_targets(pool: &PgPool, campaign_id: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar::<_, Uuid>("SELECT target_node_id FROM jobs WHERE campaign_id = $1")
        .bind(campaign_id)
        .fetch_all(pool)
        .await
        .expect("list campaign targets")
}

#[sqlx::test(migrations = "./migrations")]
async fn unprepared_campaign_never_enumerates_candidates(pool: PgPool) {
    seed_candidate(&pool, "inert", "application/pdf", 1024).await;
    let draft = create_backfill_campaign(&pool, &request(100))
        .await
        .expect("create draft campaign");
    assert_eq!(draft.state, BackfillState::Draft);

    // A draft campaign has no frozen snapshot boundary, so refilling it must be
    // a no-op even if the coordinator is somehow asked to run it.
    let (enqueued, exhausted) =
        enqueue_ocr_backfill_batch(&pool, &draft, &mimes(), Some(ENGINE), 100)
            .await
            .expect("refill draft campaign");
    assert_eq!(enqueued, 0);
    assert!(!exhausted);
    assert!(campaign_targets(&pool, draft.id).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn refill_is_bounded_advances_cursor_and_never_repeats_a_candidate(pool: PgPool) {
    let campaign = running_campaign(&pool, 2).await;
    for index in 0..5 {
        seed_candidate(&pool, &format!("batch-{index}"), "image/png", 2048).await;
    }

    // Batch one: the allowance bounds the enqueue below the candidate count.
    let (first, exhausted) =
        enqueue_ocr_backfill_batch(&pool, &campaign, &mimes(), Some(ENGINE), 2)
            .await
            .expect("first batch");
    assert_eq!(first, 2);
    assert!(!exhausted, "more candidates remain");

    let after_first = get_backfill_campaign(&pool, campaign.id)
        .await
        .expect("reload campaign")
        .expect("campaign exists");
    assert_eq!(after_first.enqueued_count, 2);
    assert!(after_first.cursor_created_at.is_some(), "cursor advanced");
    assert!(after_first.cursor_node_id.is_some(), "cursor advanced");

    // Batch two resumes from the durable cursor rather than rescanning.
    let (second, _) = enqueue_ocr_backfill_batch(&pool, &after_first, &mimes(), Some(ENGINE), 2)
        .await
        .expect("second batch");
    assert_eq!(second, 2);

    // Batch three exhausts the candidate set and reports it.
    let after_second = get_backfill_campaign(&pool, campaign.id)
        .await
        .expect("reload campaign")
        .expect("campaign exists");
    let (third, exhausted) =
        enqueue_ocr_backfill_batch(&pool, &after_second, &mimes(), Some(ENGINE), 2)
            .await
            .expect("third batch");
    assert_eq!(third, 1);
    assert!(exhausted, "final short batch must report exhaustion");

    let targets = campaign_targets(&pool, campaign.id).await;
    assert_eq!(targets.len(), 5);
    let mut deduped = targets.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(deduped.len(), 5, "a candidate was enqueued twice");
}

#[sqlx::test(migrations = "./migrations")]
async fn backfill_jobs_carry_campaign_origin_and_heavy_resource_class(pool: PgPool) {
    let campaign = running_campaign(&pool, 10).await;
    seed_candidate(&pool, "classified", "application/pdf", 4096).await;
    let (enqueued, _) = enqueue_ocr_backfill_batch(&pool, &campaign, &mimes(), Some(ENGINE), 10)
        .await
        .expect("refill");
    assert_eq!(enqueued, 1);

    let row = sqlx::query_as::<_, (JobType, JobOrigin, JobResourceClass, i32, Option<Uuid>)>(
        r"
        SELECT job_type, origin, resource_class, priority, campaign_id
        FROM jobs WHERE campaign_id = $1
        ",
    )
    .bind(campaign.id)
    .fetch_one(&pool)
    .await
    .expect("load enqueued backfill job");
    assert_eq!(row.0, JobType::Ocr);
    assert_eq!(row.1, JobOrigin::Backfill);
    assert_eq!(row.2, JobResourceClass::HeavyCpu);
    assert_eq!(row.3, -100, "historical work must not outrank live work");
    assert_eq!(row.4, Some(campaign.id));
}

#[sqlx::test(migrations = "./migrations")]
async fn candidates_with_active_jobs_are_excluded_before_the_batch_limit(pool: PgPool) {
    let campaign = running_campaign(&pool, 10).await;
    let busy = seed_candidate(&pool, "busy", "image/tiff", 8192).await;
    let free = seed_candidate(&pool, "free", "image/tiff", 8192).await;

    // Foreground OCR already owns `busy`; the campaign must skip it and still
    // reach `free` rather than reporting a batch that is silently one short.
    enqueue_job_with_context(
        &pool,
        JobType::Ocr,
        busy,
        100,
        JobOrigin::Foreground,
        None,
        JobResourceClass::HeavyCpu,
    )
    .await
    .expect("enqueue foreground job");

    let (enqueued, _) = enqueue_ocr_backfill_batch(&pool, &campaign, &mimes(), Some(ENGINE), 10)
        .await
        .expect("refill");
    assert_eq!(enqueued, 1);

    let targets = campaign_targets(&pool, campaign.id).await;
    assert_eq!(targets, vec![free], "eligible candidate was skipped");
    assert!(
        !targets.contains(&busy),
        "candidate with active foreground work was duplicated"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn files_created_after_the_snapshot_stay_foreground_work(pool: PgPool) {
    let campaign = running_campaign(&pool, 10).await;
    let late = seed_candidate(&pool, "late", "application/pdf", 512).await;
    sqlx::query("UPDATE nodes SET created_at = $2 WHERE id = $1")
        .bind(late)
        .bind(Utc::now() + Duration::hours(1))
        .execute(&pool)
        .await
        .expect("push node past the snapshot boundary");

    let (enqueued, _) = enqueue_ocr_backfill_batch(&pool, &campaign, &mimes(), Some(ENGINE), 10)
        .await
        .expect("refill");
    assert_eq!(enqueued, 0, "campaign enlarged itself past its snapshot");
    assert!(campaign_targets(&pool, campaign.id).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn text_from_the_current_engine_is_not_a_candidate(pool: PgPool) {
    let campaign = running_campaign(&pool, 10).await;
    let done = seed_candidate(&pool, "done", "application/pdf", 2048).await;
    let stale = seed_candidate(&pool, "stale", "application/pdf", 2048).await;
    let embedded = seed_candidate(&pool, "embedded", "application/pdf", 2048).await;
    for (node_id, version, status) in [
        (done, ENGINE, "completed"),
        (stale, "tesseract-4.0.0-old", "completed"),
        (embedded, "tesseract-4.0.0-old", "skipped"),
    ] {
        sqlx::query(
            r"
            INSERT INTO document_text (node_id, source, status, language, engine_name,
                                       engine_version, char_count)
            VALUES ($1, 'ocr', $3::document_text_status, 'eng', 'tesseract', $2, 10)
            ",
        )
        .bind(node_id)
        .bind(version)
        .bind(status)
        .execute(&pool)
        .await
        .expect("seed document text");
    }

    let (enqueued, _) = enqueue_ocr_backfill_batch(&pool, &campaign, &mimes(), Some(ENGINE), 10)
        .await
        .expect("refill");
    assert_eq!(enqueued, 1, "only the stale-engine file is a candidate");
    assert_eq!(campaign_targets(&pool, campaign.id).await, vec![stale]);
}

#[sqlx::test(migrations = "./migrations")]
async fn preflight_reports_candidates_without_enqueueing(pool: PgPool) {
    seed_candidate(&pool, "preflight", "application/pdf", 10_000).await;
    seed_candidate(&pool, "preflight", "image/jpeg", 20_000).await;
    // Not an OCR input, so it must not appear in any family bucket.
    seed_candidate(&pool, "excluded", "video/mp4", 90_000).await;
    // Finalized but with no metadata row yet: counted separately, never guessed.
    let awaiting = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(awaiting)
        .bind(ROOT_NODE_ID)
        .bind(format!("awaiting-{awaiting}"))
        .execute(&pool)
        .await
        .expect("create awaiting node");
    sqlx::query(
        r"
        INSERT INTO file_objects (id, node_id, storage_key, byte_size, checksum_sha256,
                                  upload_state)
        VALUES ($1, $2, $3, 4096, $4, 'finalized')
        ",
    )
    .bind(Uuid::new_v4())
    .bind(awaiting)
    .bind(format!("originals/{awaiting}"))
    .bind("0".repeat(64))
    .execute(&pool)
    .await
    .expect("create awaiting file object");

    let report = ocr_preflight_report(
        &pool,
        &mimes(),
        Utc::now() + Duration::minutes(5),
        Some(ENGINE),
    )
    .await
    .expect("preflight report");

    assert_eq!(report.candidates, 2, "only OCR inputs are candidates");
    assert_eq!(report.total_candidate_bytes, 30_000);
    assert_eq!(report.awaiting_metadata, 1);
    assert_eq!(report.families.len(), 2);
    assert!(
        report
            .families
            .iter()
            .all(|family| family.detected_mime != "video/mp4"),
        "non-OCR MIME leaked into the report"
    );
    for family in &report.families {
        assert!(family.p50_bytes <= family.max_bytes);
        assert!(family.p95_bytes <= family.max_bytes);
    }

    let jobs = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM jobs WHERE job_type = 'ocr'")
        .fetch_one(&pool)
        .await
        .expect("post-preflight ocr job count");
    assert_eq!(jobs, 0, "preflight enqueued work");
}
