use sqlx::PgPool;

#[sqlx::test(migrations = "./migrations")]
async fn queue_hot_paths_and_active_uniqueness_are_indexed(pool: PgPool) {
    let indexes = sqlx::query_as::<_, (String, String)>(
        "SELECT indexname, indexdef FROM pg_indexes \
         WHERE schemaname = 'public' AND indexname = ANY($1) ORDER BY indexname",
    )
    .bind([
        "jobs_active_type_target_unique",
        "jobs_claim_pending_idx",
        "jobs_expired_lease_idx",
    ])
    .fetch_all(&pool)
    .await
    .expect("load queue indexes");

    assert_eq!(indexes.len(), 3);
    let definitions = indexes
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
    assert!(definitions["jobs_active_type_target_unique"].contains("UNIQUE INDEX"));
    assert!(definitions["jobs_claim_pending_idx"].contains("WHERE (state = 'pending'"));
    assert!(definitions["jobs_expired_lease_idx"].contains("WHERE (state = 'leased'"));
}
