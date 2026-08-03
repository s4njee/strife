use chrono::Duration;
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    JobState, JobType, MIGRATOR, ROOT_NODE_ID, claim_job, complete_job, enqueue_import_scan,
    enqueue_job, fail_job, release_expired_leases, renew_job_lease,
};
use uuid::Uuid;

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .expect("connect to integration-test PostgreSQL");
    MIGRATOR.run(&pool).await.expect("apply migrations");
    Some(pool)
}

#[tokio::test]
async fn queue_is_idempotent_and_supports_completion_and_retry() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("job-test-{node_id}"))
        .execute(&pool)
        .await
        .expect("create file node");

    let enqueued = enqueue_job(&pool, JobType::MetadataExtraction, node_id, 10)
        .await
        .expect("enqueue")
        .expect("new job");
    assert!(
        enqueue_job(&pool, JobType::MetadataExtraction, node_id, 10)
            .await
            .expect("idempotent enqueue")
            .is_none()
    );

    let leased = claim_job(
        &pool,
        JobType::MetadataExtraction,
        "worker-a",
        Duration::minutes(1),
    )
    .await
    .expect("claim")
    .expect("leased job");
    assert_eq!(leased.id, enqueued.id);
    assert_eq!(leased.attempts, 1);
    assert_eq!(leased.state, JobState::Leased);
    assert!(
        renew_job_lease(&pool, leased.id, "worker-a", Duration::minutes(2))
            .await
            .expect("renew lease")
    );

    let retry = fail_job(&pool, leased.id, "temporary failure")
        .await
        .expect("fail attempt")
        .expect("updated job");
    assert_eq!(retry.state, JobState::Pending);
    assert_eq!(retry.last_error.as_deref(), Some("temporary failure"));

    let leased = claim_job(
        &pool,
        JobType::MetadataExtraction,
        "worker-a",
        Duration::minutes(1),
    )
    .await
    .expect("claim retry")
    .expect("leased retry");
    let completed = complete_job(&pool, leased.id)
        .await
        .expect("complete")
        .expect("completed job");
    assert_eq!(completed.state, JobState::Completed);
    assert!(completed.completed_at.is_some());

    let expiring = enqueue_job(&pool, JobType::PreviewGeneration, node_id, 0)
        .await
        .expect("enqueue expiring job")
        .expect("new expiring job");
    claim_job(
        &pool,
        JobType::PreviewGeneration,
        "worker-b",
        Duration::seconds(-1),
    )
    .await
    .expect("claim expiring job")
    .expect("leased expiring job");
    // Other suites may leave expired leases; assert ours is recovered.
    let released = release_expired_leases(&pool).await.expect("release lease");
    assert!(released >= 1);
    let state: JobState = sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1")
        .bind(expiring.id)
        .fetch_one(&pool)
        .await
        .expect("load released state");
    assert_eq!(state, JobState::Pending);

    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("clean up node and jobs");
}

#[tokio::test]
async fn import_scan_queue_is_source_scoped_and_idempotent() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let source_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO import_sources (id, watch_path, destination_folder_id) VALUES ($1, $2, $3)",
    )
    .bind(source_id)
    .bind(format!("/tmp/strife-job-source-{source_id}"))
    .bind(ROOT_NODE_ID)
    .execute(&pool)
    .await
    .expect("create import source");

    let first = enqueue_import_scan(&pool, source_id)
        .await
        .expect("enqueue import scan")
        .expect("enabled source");
    let repeated = enqueue_import_scan(&pool, source_id)
        .await
        .expect("repeat import scan")
        .expect("enabled source");
    assert_eq!(repeated.id, first.id);
    assert_eq!(first.job_type, JobType::ImportScan);
    assert_eq!(first.import_source_id, Some(source_id));

    sqlx::query("DELETE FROM import_sources WHERE id = $1")
        .bind(source_id)
        .execute(&pool)
        .await
        .expect("clean up source and job");
}

#[tokio::test]
async fn ocr_queue_is_unique_uses_extended_attempts_and_renews_to_completion() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let mut ocr_lock = pool.begin().await.expect("begin OCR test lock");
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(0x4f43_5200_i64)
        .execute(&mut *ocr_lock)
        .await
        .expect("acquire OCR test lock");
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("ocr-job-test-{node_id}"))
        .execute(&pool)
        .await
        .expect("create OCR target");

    let queued = enqueue_job(&pool, JobType::Ocr, node_id, -10)
        .await
        .expect("enqueue OCR")
        .expect("new OCR job");
    assert_eq!(queued.max_attempts, 5);
    assert!(
        enqueue_job(&pool, JobType::Ocr, node_id, -10)
            .await
            .expect("idempotent OCR enqueue")
            .is_none(),
        "the generic active-job index must reject duplicate OCR work"
    );

    let base_ttl = Duration::milliseconds(40);
    let leased = claim_job(&pool, JobType::Ocr, "ocr-worker", base_ttl)
        .await
        .expect("claim OCR")
        .expect("leased OCR job");
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    assert!(
        renew_job_lease(&pool, leased.id, "ocr-worker", Duration::milliseconds(100))
            .await
            .expect("renew OCR lease")
    );
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    let completed = complete_job(&pool, leased.id)
        .await
        .expect("complete OCR")
        .expect("OCR remained leased beyond its initial TTL");
    assert_eq!(completed.state, JobState::Completed);

    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("clean up OCR target");
}
