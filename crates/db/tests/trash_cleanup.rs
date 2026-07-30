use chrono::{Duration, Utc};
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    JobType, MIGRATOR, ROOT_NODE_ID, create_folder, enqueue_expired_trash_deletions, get_job,
    list_expired_trash, trash_node,
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

async fn cleanup_tree(pool: &PgPool, root_id: Uuid) {
    sqlx::query(
        r"
        WITH RECURSIVE tree AS (
            SELECT id FROM nodes WHERE id = $1
            UNION ALL
            SELECT child.id FROM nodes AS child
            JOIN tree AS parent ON child.parent_id = parent.id
        )
        DELETE FROM nodes WHERE id IN (SELECT id FROM tree)
        ",
    )
    .bind(root_id)
    .execute(pool)
    .await
    .ok();
}

#[tokio::test]
async fn expired_trash_is_enqueued_for_permanent_deletion() {
    // Serialize against other trash-cleanup tests that share the expired-trash queue.
    let _lock = CLEANUP_TEST_LOCK.lock().await;
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };

    let parent = create_folder(
        &pool,
        ROOT_NODE_ID,
        &format!("cleanup-parent-{}", Uuid::new_v4()),
    )
    .await
    .expect("create parent");
    let folder = create_folder(&pool, parent.id, "Expired")
        .await
        .expect("create folder");
    trash_node(&pool, folder.id).await.expect("trash");

    // Backdate purge time into the past.
    sqlx::query(
        r"
        UPDATE trash_entries
        SET scheduled_purge_at = $2, trashed_at = $2
        WHERE node_id = $1
        ",
    )
    .bind(folder.id)
    .bind(Utc::now() - Duration::days(31))
    .execute(&pool)
    .await
    .expect("backdate purge");

    let expired = list_expired_trash(&pool, 50).await.expect("list expired");
    assert!(expired.iter().any(|entry| entry.node_id == folder.id));

    let first = enqueue_expired_trash_deletions(&pool, 50)
        .await
        .expect("enqueue first pass");
    assert!(first >= 1);

    let jobs = sqlx::query_scalar::<_, Uuid>(
        r"
        SELECT id FROM jobs
        WHERE target_node_id = $1 AND job_type = $2 AND state IN ('pending', 'leased')
        ",
    )
    .bind(folder.id)
    .bind(JobType::PermanentDeletion)
    .fetch_all(&pool)
    .await
    .expect("list jobs");
    assert_eq!(jobs.len(), 1, "our expired node must receive a deletion job");
    let job = get_job(&pool, jobs[0]).await.expect("get job").expect("exists");
    assert_eq!(job.job_type, JobType::PermanentDeletion);

    let second = enqueue_expired_trash_deletions(&pool, 50)
        .await
        .expect("enqueue second pass");
    // May still enqueue other leftover expired rows, but not a second job for us.
    let jobs_again = sqlx::query_scalar::<_, i64>(
        r"
        SELECT COUNT(*) FROM jobs
        WHERE target_node_id = $1 AND job_type = $2 AND state IN ('pending', 'leased')
        ",
    )
    .bind(folder.id)
    .bind(JobType::PermanentDeletion)
    .fetch_one(&pool)
    .await
    .expect("count jobs");
    assert_eq!(jobs_again, 1, "cleanup must not duplicate jobs for the same node");
    let _ = second;

    // Clean up job and tree.
    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(job.id)
        .execute(&pool)
        .await
        .ok();
    cleanup_tree(&pool, parent.id).await;
}

static CLEANUP_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn cleanup_batch_respects_limit() {
    let _lock = CLEANUP_TEST_LOCK.lock().await;
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };

    let parent = create_folder(
        &pool,
        ROOT_NODE_ID,
        &format!("cleanup-batch-{}", Uuid::new_v4()),
    )
    .await
    .expect("create parent");

    let mut node_ids = Vec::new();
    for i in 0..3 {
        let folder = create_folder(&pool, parent.id, &format!("item-{i}"))
            .await
            .expect("create");
        trash_node(&pool, folder.id).await.expect("trash");
        sqlx::query(
            "UPDATE trash_entries SET scheduled_purge_at = $2, trashed_at = $2 WHERE node_id = $1",
        )
        .bind(folder.id)
        .bind(Utc::now() - Duration::days(40 + i64::from(i)))
        .execute(&pool)
        .await
        .expect("backdate");
        node_ids.push(folder.id);
    }

    // Push every other expired entry into the future so this batch is isolated.
    sqlx::query(
        r"
        UPDATE trash_entries
        SET scheduled_purge_at = now() + INTERVAL '30 days'
        WHERE node_id <> ALL($1)
          AND scheduled_purge_at <= now()
        ",
    )
    .bind(&node_ids)
    .execute(&pool)
    .await
    .expect("isolate expired set");

    sqlx::query(
        "DELETE FROM jobs WHERE target_node_id = ANY($1) AND job_type = $2 AND state IN ('pending', 'leased')",
    )
    .bind(&node_ids)
    .bind(JobType::PermanentDeletion)
    .execute(&pool)
    .await
    .expect("clear jobs");

    let limited = list_expired_trash(&pool, 10).await.expect("list limited");
    assert_eq!(limited.len(), 3);
    assert!(limited.iter().all(|entry| node_ids.contains(&entry.node_id)));

    let enqueued = enqueue_expired_trash_deletions(&pool, 2)
        .await
        .expect("enqueue limited");
    assert_eq!(enqueued, 2, "batch size must cap newly enqueued jobs");

    let with_jobs = sqlx::query_scalar::<_, i64>(
        r"
        SELECT COUNT(*) FROM jobs
        WHERE target_node_id = ANY($1)
          AND job_type = $2
          AND state IN ('pending', 'leased')
        ",
    )
    .bind(&node_ids)
    .bind(JobType::PermanentDeletion)
    .fetch_one(&pool)
    .await
    .expect("count jobs for fixture");
    assert_eq!(with_jobs, 2);

    let second = enqueue_expired_trash_deletions(&pool, 2)
        .await
        .expect("enqueue remainder");
    assert_eq!(second, 1, "remaining expired fixture should enqueue next");

    sqlx::query("DELETE FROM jobs WHERE target_node_id = ANY($1)")
        .bind(&node_ids)
        .execute(&pool)
        .await
        .ok();
    cleanup_tree(&pool, parent.id).await;
}
