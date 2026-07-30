use chrono::Duration;
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    JobState, JobType, MIGRATOR, ROOT_NODE_ID, claim_job, complete_job, enqueue_job, fail_job,
    release_expired_leases,
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
