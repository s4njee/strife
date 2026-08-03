//! Job retention: what gets purged, what is kept, and what is never touched.
//!
//! The jobs table is append-only in practice — every upload, import, extraction,
//! and backfill batch adds a row — so without a purge it grows for the life of
//! the deployment. These tests pin the policy that decides what may go.

use sqlx::PgPool;
use strife_db::{
    DEFAULT_IMPORT_SOURCE_ID, JobType, ROOT_NODE_ID, enqueue_job, get_job, purge_expired_jobs,
};
use uuid::Uuid;

async fn node(pool: &PgPool) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("retention-{node_id}"))
        .execute(pool)
        .await
        .expect("create node");
    node_id
}

/// Creates one job in a chosen state, aged by a number of days.
async fn aged_job(pool: &PgPool, state: &str, age_days: i32) -> Uuid {
    let node_id = node(pool).await;
    let job = enqueue_job(pool, JobType::MetadataExtraction, node_id, 0)
        .await
        .expect("enqueue")
        .expect("new job");
    sqlx::query(
        "UPDATE jobs
         SET state = $2::job_state,
             completed_at = now() - ($3 * interval '1 day'),
             updated_at = now() - ($3 * interval '1 day')
         WHERE id = $1",
    )
    .bind(job.id)
    .bind(state)
    .bind(age_days)
    .execute(pool)
    .await
    .expect("age job");
    job.id
}

#[sqlx::test(migrations = "./migrations")]
async fn old_successes_are_purged_and_recent_ones_are_kept(pool: PgPool) {
    let old = aged_job(&pool, "completed", 30).await;
    let recent = aged_job(&pool, "completed", 1).await;
    let skipped_old = aged_job(&pool, "skipped", 30).await;

    let removed = purge_expired_jobs(&pool, 7, 30, 500).await.expect("purge");
    assert_eq!(removed, 2);

    assert!(get_job(&pool, old).await.expect("load").is_none());
    assert!(get_job(&pool, skipped_old).await.expect("load").is_none());
    // A completed job younger than the window is still a receipt someone may
    // be looking at in the console.
    assert!(get_job(&pool, recent).await.expect("load").is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn failures_are_kept_longer_than_successes(pool: PgPool) {
    // Same age, different outcome: past the success window, inside the failure
    // window. A failure carries last_error and the attempt count, which is what
    // triage reads; a success carries nothing anyone re-reads.
    let completed = aged_job(&pool, "completed", 10).await;
    let failed = aged_job(&pool, "failed", 10).await;
    let cancelled = aged_job(&pool, "cancelled", 10).await;

    let removed = purge_expired_jobs(&pool, 7, 30, 500).await.expect("purge");
    assert_eq!(removed, 1);
    assert!(get_job(&pool, completed).await.expect("load").is_none());
    assert!(get_job(&pool, failed).await.expect("load").is_some());
    assert!(get_job(&pool, cancelled).await.expect("load").is_some());

    // Past the failure window too, they go.
    sqlx::query("UPDATE jobs SET completed_at = now() - interval '60 days'")
        .execute(&pool)
        .await
        .expect("age further");
    let removed = purge_expired_jobs(&pool, 7, 30, 500).await.expect("purge");
    assert_eq!(removed, 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn pending_and_leased_jobs_are_never_purged_at_any_age(pool: PgPool) {
    let pending = aged_job(&pool, "pending", 3650).await;
    let leased = aged_job(&pool, "leased", 3650).await;

    let removed = purge_expired_jobs(&pool, 1, 1, 500).await.expect("purge");
    assert_eq!(removed, 0, "unfinished work was deleted");

    // A leased job whose worker died is recovered by lease expiry. Deleting it
    // would strand the work with nothing left to recover from.
    assert!(get_job(&pool, pending).await.expect("load").is_some());
    assert!(get_job(&pool, leased).await.expect("load").is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_failure_behind_an_unresolved_import_error_is_retained(pool: PgPool) {
    let node_id = node(&pool).await;
    let job = enqueue_job(&pool, JobType::MetadataExtraction, node_id, 0)
        .await
        .expect("enqueue")
        .expect("new job");
    sqlx::query(
        "UPDATE jobs SET state = 'failed', last_error = 'extractor exploded',
                completed_at = now() - interval '365 days' WHERE id = $1",
    )
    .bind(job.id)
    .execute(&pool)
    .await
    .expect("age job");

    // The Actionable Errors tab lists failed import entries and offers a retry.
    sqlx::query(
        "INSERT INTO import_entries
             (id, source_id, source_path, source_size, source_modified_at, state,
              resulting_node_id)
         VALUES ($1, $2, $3, 10, now(), 'failed', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(DEFAULT_IMPORT_SOURCE_ID)
    .bind(format!("inbox/{node_id}.bin"))
    .bind(node_id)
    .execute(&pool)
    .await
    .expect("create failed import entry");

    let removed = purge_expired_jobs(&pool, 7, 30, 500).await.expect("purge");
    assert_eq!(removed, 0, "a job a user can still act on was deleted");
    assert!(get_job(&pool, job.id).await.expect("load").is_some());

    // Once the import is resolved, the job is ordinary history again.
    sqlx::query("UPDATE import_entries SET state = 'imported' WHERE resulting_node_id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("resolve entry");
    let removed = purge_expired_jobs(&pool, 7, 30, 500).await.expect("purge");
    assert_eq!(removed, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_purge_is_bounded_by_its_batch_size(pool: PgPool) {
    for _ in 0..5 {
        aged_job(&pool, "completed", 30).await;
    }
    // A table neglected for months must drain in bites rather than in one
    // statement that locks it.
    let first = purge_expired_jobs(&pool, 7, 30, 2).await.expect("purge");
    assert_eq!(first, 2);
    let second = purge_expired_jobs(&pool, 7, 30, 2).await.expect("purge");
    assert_eq!(second, 2);
    let third = purge_expired_jobs(&pool, 7, 30, 2).await.expect("purge");
    assert_eq!(third, 1);
    let fourth = purge_expired_jobs(&pool, 7, 30, 2).await.expect("purge");
    assert_eq!(fourth, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn purging_a_job_leaves_its_node_and_artifacts_alone(pool: PgPool) {
    let node_id = node(&pool).await;
    let job = enqueue_job(&pool, JobType::MetadataExtraction, node_id, 0)
        .await
        .expect("enqueue")
        .expect("new job");
    sqlx::query(
        "UPDATE jobs SET state = 'completed', completed_at = now() - interval '30 days'
         WHERE id = $1",
    )
    .bind(job.id)
    .execute(&pool)
    .await
    .expect("age job");

    purge_expired_jobs(&pool, 7, 30, 500).await.expect("purge");

    // The job is bookkeeping; the file it processed is the point. Deleting the
    // receipt must never cascade into the thing it describes.
    let node_exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM nodes WHERE id = $1)")
        .bind(node_id)
        .fetch_one(&pool)
        .await
        .expect("check node");
    assert!(node_exists, "purging a job removed its node");
}
