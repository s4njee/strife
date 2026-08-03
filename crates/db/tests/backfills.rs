use chrono::{Duration, Utc};
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    BackfillKind, BackfillState, JobOrigin, JobResourceClass, JobState, JobType, MIGRATOR,
    NewBackfillCampaign, ROOT_NODE_ID, claim_job_with_resource_lease, complete_job,
    create_backfill_campaign, enqueue_job, enqueue_job_with_context, get_backfill_refill_window,
    prepare_backfill_campaign, transition_backfill_campaign,
};
use uuid::Uuid;

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("connect to integration-test PostgreSQL");
    MIGRATOR.run(&pool).await.expect("apply migrations");
    Some(pool)
}

fn request() -> NewBackfillCampaign {
    NewBackfillCampaign {
        kind: BackfillKind::Ocr,
        candidate_definition: serde_json::json!({"version": 1, "engine": "tesseract"}),
        batch_size: 100,
        max_queued: 500,
        max_running: 1,
        resource_class: JobResourceClass::HeavyCpu,
        foreground_fairness: 20,
        created_by_version: "test".to_owned(),
    }
}

#[tokio::test]
async fn campaign_is_inert_until_prepared_and_explicitly_resumed() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let campaign = create_backfill_campaign(&pool, &request())
        .await
        .expect("create campaign");
    assert_eq!(campaign.state, BackfillState::Draft);
    assert!(
        get_backfill_refill_window(&pool, campaign.id)
            .await
            .expect("draft refill window")
            .is_none()
    );

    let prepared = prepare_backfill_campaign(&pool, campaign.id, 700_000, Utc::now(), None)
        .await
        .expect("prepare")
        .expect("draft campaign");
    assert_eq!(prepared.state, BackfillState::Paused);
    assert_eq!(prepared.candidate_count, 700_000);
    assert!(
        get_backfill_refill_window(&pool, campaign.id)
            .await
            .expect("paused refill window")
            .is_none()
    );

    transition_backfill_campaign(&pool, campaign.id, BackfillState::Running, Some("canary"))
        .await
        .expect("resume")
        .expect("allowed transition");
    let window = get_backfill_refill_window(&pool, campaign.id)
        .await
        .expect("running refill window")
        .expect("running campaign");
    assert_eq!(window.allowance, 100);

    sqlx::query("DELETE FROM backfill_campaigns WHERE id = $1")
        .bind(campaign.id)
        .execute(&pool)
        .await
        .expect("clean up campaign");
}

#[tokio::test]
async fn cancellation_skips_pending_campaign_jobs_without_touching_files() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let campaign = create_backfill_campaign(&pool, &request())
        .await
        .expect("create campaign");
    prepare_backfill_campaign(&pool, campaign.id, 1, Utc::now(), None)
        .await
        .expect("prepare");
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("backfill-cancel-{node_id}"))
        .execute(&pool)
        .await
        .expect("create node");
    let job = enqueue_job_with_context(
        &pool,
        JobType::Ocr,
        node_id,
        -100,
        JobOrigin::Backfill,
        Some(campaign.id),
        JobResourceClass::HeavyCpu,
    )
    .await
    .expect("enqueue")
    .expect("new campaign job");

    let cancelled = transition_backfill_campaign(
        &pool,
        campaign.id,
        BackfillState::Cancelled,
        Some("operator cancelled"),
    )
    .await
    .expect("cancel")
    .expect("allowed transition");
    assert_eq!(cancelled.state, BackfillState::Cancelled);
    let state: JobState = sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1")
        .bind(job.id)
        .fetch_one(&pool)
        .await
        .expect("load job state");
    assert_eq!(state, JobState::Skipped);
    let node_exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM nodes WHERE id = $1)")
        .bind(node_id)
        .fetch_one(&pool)
        .await
        .expect("load node");
    assert!(node_exists);

    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("clean up node");
    sqlx::query("DELETE FROM backfill_campaigns WHERE id = $1")
        .bind(campaign.id)
        .execute(&pool)
        .await
        .expect("clean up campaign");
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn claims_respect_pause_fairness_and_shared_resource_capacity() {
    let Some(pool) = test_pool().await else {
        return;
    };
    // Make retries deterministic after an interrupted prior test run.
    sqlx::query("DELETE FROM nodes WHERE name LIKE 'backfill-fairness-%'")
        .execute(&pool)
        .await
        .expect("clean stale nodes");
    sqlx::query(
        "DELETE FROM backfill_campaigns WHERE created_by_version = 'test' AND kind = 'attachment_text'",
    )
    .execute(&pool)
    .await
    .expect("clean stale campaigns");
    sqlx::query("DELETE FROM job_claim_fairness WHERE job_type = 'metadata_extraction'")
        .execute(&pool)
        .await
        .expect("reset isolated fairness counter");
    let mut settings = request();
    settings.kind = BackfillKind::AttachmentText;
    settings.resource_class = JobResourceClass::Extractor;
    settings.foreground_fairness = 2;
    let campaign = create_backfill_campaign(&pool, &settings)
        .await
        .expect("create campaign");
    prepare_backfill_campaign(&pool, campaign.id, 4, Utc::now(), None)
        .await
        .expect("prepare");

    let mut node_ids = Vec::new();
    for sequence in 0..4 {
        let node_id = Uuid::new_v4();
        sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
            .bind(node_id)
            .bind(ROOT_NODE_ID)
            .bind(format!("backfill-fairness-{sequence}-{node_id}"))
            .execute(&pool)
            .await
            .expect("create node");
        node_ids.push(node_id);
    }
    enqueue_job_with_context(
        &pool,
        JobType::MetadataExtraction,
        node_ids[0],
        -100,
        JobOrigin::Backfill,
        Some(campaign.id),
        JobResourceClass::Extractor,
    )
    .await
    .expect("enqueue backfill")
    .expect("new backfill job");
    let paused_claim = claim_job_with_resource_lease(
        &pool,
        JobType::MetadataExtraction,
        "worker-paused",
        Duration::minutes(1),
    )
    .await
    .expect("claim while paused");
    if let Some(job) = paused_claim {
        assert_ne!(job.campaign_id, Some(campaign.id));
        complete_job(&pool, job.id)
            .await
            .expect("release unrelated fixture job");
    }
    let paused_state: JobState =
        sqlx::query_scalar("SELECT state FROM jobs WHERE campaign_id = $1 AND target_node_id = $2")
            .bind(campaign.id)
            .bind(node_ids[0])
            .fetch_one(&pool)
            .await
            .expect("load paused job");
    assert_eq!(paused_state, JobState::Pending);
    sqlx::query(
        "UPDATE job_claim_fairness SET foreground_claims_since_backfill = 0 WHERE job_type = 'metadata_extraction'",
    )
    .execute(&pool)
    .await
    .expect("start fairness scenario at zero");

    transition_backfill_campaign(&pool, campaign.id, BackfillState::Running, None)
        .await
        .expect("resume");
    for node_id in &node_ids[1..] {
        enqueue_job(&pool, JobType::MetadataExtraction, *node_id, i32::MAX)
            .await
            .expect("enqueue foreground")
            .expect("new foreground job");
    }

    let mut active_foreground = Vec::new();
    for sequence in 0..2 {
        let claimed = claim_job_with_resource_lease(
            &pool,
            JobType::MetadataExtraction,
            &format!("foreground-{sequence}"),
            Duration::minutes(1),
        )
        .await
        .expect("claim foreground")
        .expect("foreground available");
        assert_eq!(claimed.origin, JobOrigin::Foreground);
        active_foreground.push(claimed);
    }
    assert!(
        claim_job_with_resource_lease(
            &pool,
            JobType::MetadataExtraction,
            "capacity-check",
            Duration::minutes(1),
        )
        .await
        .expect("capacity check")
        .is_none(),
        "the two extractor slots must be authoritative across owners"
    );
    for claimed in active_foreground {
        complete_job(&pool, claimed.id)
            .await
            .expect("complete foreground");
    }
    let backfill = claim_job_with_resource_lease(
        &pool,
        JobType::MetadataExtraction,
        "fairness-worker",
        Duration::minutes(1),
    )
    .await
    .expect("claim after fairness budget")
    .expect("backfill available");
    assert_eq!(backfill.origin, JobOrigin::Backfill);
    complete_job(&pool, backfill.id)
        .await
        .expect("complete backfill");

    sqlx::query("DELETE FROM nodes WHERE id = ANY($1)")
        .bind(&node_ids)
        .execute(&pool)
        .await
        .expect("clean up nodes");
    sqlx::query("DELETE FROM backfill_campaigns WHERE id = $1")
        .bind(campaign.id)
        .execute(&pool)
        .await
        .expect("clean up campaign");
}
