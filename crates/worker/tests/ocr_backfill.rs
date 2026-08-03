use chrono::{Duration, Utc};
use sqlx::PgPool;
use std::sync::Arc;
use strife_db::{
    BackfillKind, BackfillState, JobResourceClass, NewBackfillCampaign, ROOT_NODE_ID,
    create_backfill_campaign, prepare_backfill_campaign, transition_backfill_campaign,
};
use strife_worker::{BackfillCoordinator, OcrBackfillProvider};
use uuid::Uuid;

async fn seed_candidate(pool: &PgPool, mime: &str) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("candidate-{node_id}"))
        .execute(pool)
        .await
        .expect("create candidate node");
    sqlx::query(
        r"
        INSERT INTO file_objects (id, node_id, storage_key, byte_size, checksum_sha256,
                                  upload_state)
        VALUES ($1, $2, $3, 1024, $4, 'finalized')
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .bind(format!("originals/{node_id}"))
    .bind("0".repeat(64))
    .execute(pool)
    .await
    .expect("create file object");
    sqlx::query("INSERT INTO node_metadata (node_id, detected_mime, media_kind) VALUES ($1, $2, 'document')")
        .bind(node_id)
        .bind(mime)
        .execute(pool)
        .await
        .expect("create node metadata");
    node_id
}

async fn set_engine(pool: &PgPool) {
    strife_db::set_ocr_engine_state(pool, "tesseract", "tesseract-5.5.0-test", "eng")
        .await
        .expect("record verified OCR engine");
}

fn coordinator(pool: &PgPool) -> BackfillCoordinator {
    BackfillCoordinator::new().with_provider(
        BackfillKind::Ocr,
        Arc::new(OcrBackfillProvider::new(pool.clone())),
    )
}

async fn ocr_job_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM jobs WHERE job_type = 'ocr'")
        .fetch_one(pool)
        .await
        .expect("count ocr jobs")
}

fn request() -> NewBackfillCampaign {
    NewBackfillCampaign {
        kind: BackfillKind::Ocr,
        candidate_definition: serde_json::json!({"version": 1}),
        batch_size: 100,
        max_queued: 500,
        max_running: 1,
        resource_class: JobResourceClass::HeavyCpu,
        foreground_fairness: 20,
        created_by_version: "test".to_owned(),
    }
}

#[sqlx::test(migrations = "../db/migrations")]
async fn coordinator_leaves_draft_and_paused_campaigns_untouched(pool: PgPool) {
    set_engine(&pool).await;
    seed_candidate(&pool, "application/pdf").await;

    // Draft: created but never prepared.
    create_backfill_campaign(&pool, &request())
        .await
        .expect("create draft");
    coordinator(&pool)
        .run_once(&pool)
        .await
        .expect("coordinator pass over draft");
    assert_eq!(
        ocr_job_count(&pool).await,
        0,
        "draft campaign enqueued work"
    );

    // Paused: prepared with a reviewed snapshot but never resumed.
    let prepared = create_backfill_campaign(&pool, &request())
        .await
        .expect("create second campaign");
    prepare_backfill_campaign(
        &pool,
        prepared.id,
        1,
        Utc::now() + Duration::minutes(5),
        None,
    )
    .await
    .expect("prepare")
    .expect("draft campaign");
    coordinator(&pool)
        .run_once(&pool)
        .await
        .expect("coordinator pass over paused");
    assert_eq!(
        ocr_job_count(&pool).await,
        0,
        "paused campaign enqueued work"
    );
}

#[sqlx::test(migrations = "../db/migrations")]
async fn coordinator_refills_a_running_campaign_and_drains_when_exhausted(pool: PgPool) {
    set_engine(&pool).await;
    seed_candidate(&pool, "application/pdf").await;
    seed_candidate(&pool, "image/png").await;

    let campaign = create_backfill_campaign(&pool, &request())
        .await
        .expect("create campaign");
    prepare_backfill_campaign(
        &pool,
        campaign.id,
        2,
        Utc::now() + Duration::minutes(5),
        None,
    )
    .await
    .expect("prepare")
    .expect("draft campaign");
    transition_backfill_campaign(&pool, campaign.id, BackfillState::Running, None)
        .await
        .expect("resume")
        .expect("paused campaign");

    coordinator(&pool)
        .run_once(&pool)
        .await
        .expect("first coordinator pass");
    assert_eq!(
        ocr_job_count(&pool).await,
        2,
        "candidates were not enqueued"
    );

    // A second pass finds nothing left and drains rather than spinning.
    coordinator(&pool)
        .run_once(&pool)
        .await
        .expect("second coordinator pass");
    let drained = strife_db::get_backfill_campaign(&pool, campaign.id)
        .await
        .expect("reload campaign")
        .expect("campaign exists");
    assert_eq!(drained.state, BackfillState::Draining);
    assert_eq!(ocr_job_count(&pool).await, 2, "drain enqueued extra work");
}

#[sqlx::test(migrations = "../db/migrations")]
async fn running_campaign_is_inert_without_a_verified_engine(pool: PgPool) {
    // No `set_engine` call: no worker has verified Tesseract yet.
    seed_candidate(&pool, "application/pdf").await;
    let campaign = create_backfill_campaign(&pool, &request())
        .await
        .expect("create campaign");
    prepare_backfill_campaign(
        &pool,
        campaign.id,
        1,
        Utc::now() + Duration::minutes(5),
        None,
    )
    .await
    .expect("prepare")
    .expect("draft campaign");
    transition_backfill_campaign(&pool, campaign.id, BackfillState::Running, None)
        .await
        .expect("resume")
        .expect("paused campaign");

    coordinator(&pool)
        .run_once(&pool)
        .await
        .expect("coordinator pass without engine");
    assert_eq!(
        ocr_job_count(&pool).await,
        0,
        "unknown engine version enqueued the whole library"
    );
}
