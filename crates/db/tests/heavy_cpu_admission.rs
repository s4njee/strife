//! The shared heavy-CPU permit and origin priority.
//!
//! OCR, email parsing, and attachment extraction compete for one admission
//! permit spanning every worker process. It is a set of durable lease rows
//! rather than an advisory lock, so a worker that dies mid-job frees its slot
//! when the lease expires instead of holding it until the connection drops.

use chrono::Duration;
use sqlx::PgPool;
use strife_db::{
    JobOrigin, JobResourceClass, JobType, ROOT_NODE_ID, claim_job_with_resource_lease,
    default_resource_class, enqueue_job_with_context, set_resource_slots,
};
use uuid::Uuid;

async fn node(pool: &PgPool, name: &str) -> Uuid {
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("{name}-{node_id}"))
        .execute(pool)
        .await
        .expect("create node");
    node_id
}

async fn queue(pool: &PgPool, job_type: JobType, origin: JobOrigin, priority: i32) -> Uuid {
    let node_id = node(pool, "job").await;
    enqueue_job_with_context(
        pool,
        job_type,
        node_id,
        priority,
        origin,
        None,
        default_resource_class(job_type),
    )
    .await
    .expect("enqueue")
    .expect("new job");
    node_id
}

#[sqlx::test(migrations = "./migrations")]
async fn one_permit_is_shared_across_every_heavy_extractor(pool: PgPool) {
    // Email parsing, OCR, and attachment extraction are all heavy_cpu.
    for job_type in [
        JobType::EmailExtraction,
        JobType::Ocr,
        JobType::AttachmentExtraction,
    ] {
        assert_eq!(
            default_resource_class(job_type),
            JobResourceClass::HeavyCpu,
            "{job_type:?} escaped the shared permit"
        );
        queue(&pool, job_type, JobOrigin::Foreground, 0).await;
    }

    // With one slot, exactly one of the three may run — whichever family it
    // belongs to. A per-family limit alone would let three run at once.
    let first = claim_job_with_resource_lease(
        &pool,
        JobType::EmailExtraction,
        "worker-a",
        Duration::minutes(5),
    )
    .await
    .expect("claim")
    .expect("first job leased");

    for job_type in [JobType::Ocr, JobType::AttachmentExtraction] {
        let blocked =
            claim_job_with_resource_lease(&pool, job_type, "worker-b", Duration::minutes(5))
                .await
                .expect("claim");
        assert!(
            blocked.is_none(),
            "{job_type:?} claimed work while the shared permit was held"
        );
    }
    assert_eq!(first.job_type, JobType::EmailExtraction);
}

#[sqlx::test(migrations = "./migrations")]
async fn widening_the_permit_admits_more_work(pool: PgPool) {
    for job_type in [JobType::EmailExtraction, JobType::Ocr] {
        queue(&pool, job_type, JobOrigin::Foreground, 0).await;
    }
    let widened = set_resource_slots(&pool, JobResourceClass::HeavyCpu, 2)
        .await
        .expect("widen");
    assert_eq!(widened, 2);

    // Resizing is a configuration change rather than a migration, so an
    // operator can open the permit during a quiet window and close it after.
    assert!(
        claim_job_with_resource_lease(
            &pool,
            JobType::EmailExtraction,
            "worker-a",
            Duration::minutes(5)
        )
        .await
        .expect("claim")
        .is_some()
    );
    assert!(
        claim_job_with_resource_lease(&pool, JobType::Ocr, "worker-b", Duration::minutes(5))
            .await
            .expect("claim")
            .is_some()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn shrinking_the_permit_never_revokes_a_running_slot(pool: PgPool) {
    set_resource_slots(&pool, JobResourceClass::HeavyCpu, 2)
        .await
        .expect("widen");
    queue(&pool, JobType::EmailExtraction, JobOrigin::Foreground, 0).await;
    queue(&pool, JobType::Ocr, JobOrigin::Foreground, 0).await;
    claim_job_with_resource_lease(
        &pool,
        JobType::EmailExtraction,
        "worker-a",
        Duration::minutes(5),
    )
    .await
    .expect("claim")
    .expect("leased");
    claim_job_with_resource_lease(&pool, JobType::Ocr, "worker-b", Duration::minutes(5))
        .await
        .expect("claim")
        .expect("leased");

    // Both slots are held, so a shrink cannot take either away yet. Orphaning a
    // running job to satisfy a config change would be the wrong trade.
    let after = set_resource_slots(&pool, JobResourceClass::HeavyCpu, 1)
        .await
        .expect("shrink");
    assert_eq!(after, 2, "a leased slot was revoked");
}

#[sqlx::test(migrations = "./migrations")]
async fn an_expired_lease_frees_its_slot_for_another_worker(pool: PgPool) {
    queue(&pool, JobType::EmailExtraction, JobOrigin::Foreground, 0).await;
    queue(&pool, JobType::Ocr, JobOrigin::Foreground, 0).await;

    // A worker that dies mid-job holds nothing an operator has to clear by
    // hand; the lease simply expires. An advisory lock would not behave this
    // way, which is why the permit is rows rather than a lock.
    claim_job_with_resource_lease(
        &pool,
        JobType::EmailExtraction,
        "doomed-worker",
        Duration::seconds(-1),
    )
    .await
    .expect("claim")
    .expect("leased");

    let recovered =
        claim_job_with_resource_lease(&pool, JobType::Ocr, "worker-b", Duration::minutes(5))
            .await
            .expect("claim");
    assert!(recovered.is_some(), "an expired slot was never reclaimed");
}

#[sqlx::test(migrations = "./migrations")]
async fn foreground_work_outranks_repair_which_outranks_backfill(pool: PgPool) {
    // Queued youngest-first so ordering cannot pass by accident of insert time.
    queue(&pool, JobType::MetadataExtraction, JobOrigin::Repair, 0).await;
    queue(&pool, JobType::MetadataExtraction, JobOrigin::Foreground, 0).await;

    let first = claim_job_with_resource_lease(
        &pool,
        JobType::MetadataExtraction,
        "worker-a",
        Duration::minutes(5),
    )
    .await
    .expect("claim")
    .expect("leased");
    // A user waiting on an upload must not queue behind repair work.
    assert_eq!(first.origin, JobOrigin::Foreground);
}
