//! Watched-folder import E2E: scan → import → recovery without duplicates → name conflict.

use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use strife_db::{
    DEFAULT_IMPORT_SOURCE_ID, ImportEntryState, MIGRATOR, ROOT_NODE_ID, list_import_entries,
    list_pending_entries,
};
use strife_importer::{
    PostgresDiscoverySink, ScanOptions, import_entry, recover_interrupted_imports, scan_directory,
};
use strife_storage::{DiskGuard, LocalFsBackend};
use uuid::Uuid;

const IMPORT_TEST_LOCK_KEY: i64 = 0x5354_5249_4645;

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&database_url)
        .await
        .expect("connect");
    MIGRATOR.run(&pool).await.expect("migrate");
    Some(pool)
}

async fn lock_import_suite(pool: &PgPool) -> Transaction<'_, Postgres> {
    let mut tx = pool.begin().await.expect("begin import suite lock");
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(IMPORT_TEST_LOCK_KEY)
        .execute(&mut *tx)
        .await
        .expect("acquire import suite lock");
    tx
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn import_scan_recovery_and_name_conflict() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL unset; skipping import e2e");
        return;
    };
    let _import_lock = lock_import_suite(&pool).await;

    let fixture = format!("e2e-import-{}", Uuid::new_v4());
    let watch_root = std::env::temp_dir().join(format!("strife-watch-{fixture}"));
    let storage_root = std::env::temp_dir().join(format!("strife-storage-{fixture}"));
    tokio::fs::create_dir_all(&watch_root)
        .await
        .expect("watch root");

    let file_name = format!("{fixture}.txt");
    let contents = b"import-e2e-payload";
    tokio::fs::write(watch_root.join(&file_name), contents)
        .await
        .expect("write source");

    let storage = LocalFsBackend::new(&storage_root).await.expect("storage");
    let sink = PostgresDiscoverySink::new(&pool);
    let guard = DiskGuard::new(100).expect("guard");

    // Place file, scan, import
    scan_directory(
        &watch_root,
        DEFAULT_IMPORT_SOURCE_ID,
        ScanOptions::default(),
        &sink,
    )
    .await
    .expect("scan");
    let entry = list_pending_entries(&pool, DEFAULT_IMPORT_SOURCE_ID)
        .await
        .expect("pending")
        .into_iter()
        .find(|entry| entry.source_path == file_name)
        .expect("discovered entry");
    let node = import_entry(&pool, &storage, &watch_root, ROOT_NODE_ID, &entry, guard)
        .await
        .expect("import")
        .expect("finalized node");
    assert_eq!(node.name, file_name);
    assert!(
        !watch_root.join(&file_name).exists(),
        "source removed after import"
    );

    let count_nodes = |pool: &PgPool, name: String| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                r"
                SELECT count(*) FROM nodes
                WHERE parent_id = $1 AND name = $2 AND kind = 'file' AND lifecycle_state = 'active'
                ",
            )
            .bind(ROOT_NODE_ID)
            .bind(name)
            .fetch_one(&pool)
            .await
            .expect("count")
        }
    };
    assert_eq!(count_nodes(&pool, file_name.clone()).await, 1);

    // Restart recovery twice — must never create a second node
    for _ in 0..2 {
        recover_interrupted_imports(
            &pool,
            &storage,
            &watch_root,
            ROOT_NODE_ID,
            DiskGuard::new(100).expect("guard"),
        )
        .await
        .expect("recover");
    }
    assert_eq!(
        count_nodes(&pool, file_name.clone()).await,
        1,
        "recovery must not duplicate nodes"
    );

    // Pre-existing destination name → failed import entry with clear error
    let conflict_name = format!("conflict-{fixture}.txt");
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(Uuid::new_v4())
        .bind(ROOT_NODE_ID)
        .bind(&conflict_name)
        .execute(&pool)
        .await
        .expect("seed conflicting active node");
    tokio::fs::write(watch_root.join(&conflict_name), b"conflict-payload")
        .await
        .expect("write conflict source");
    scan_directory(
        &watch_root,
        DEFAULT_IMPORT_SOURCE_ID,
        ScanOptions::default(),
        &sink,
    )
    .await
    .expect("scan conflict");
    let conflict_entry = list_pending_entries(&pool, DEFAULT_IMPORT_SOURCE_ID)
        .await
        .expect("pending conflict")
        .into_iter()
        .find(|entry| entry.source_path == conflict_name)
        .expect("conflict entry discovered");
    let published = import_entry(
        &pool,
        &storage,
        &watch_root,
        ROOT_NODE_ID,
        &conflict_entry,
        DiskGuard::new(100).expect("guard"),
    )
    .await;
    match published {
        Ok(None) => {}
        Ok(Some(_)) => panic!("name conflict must not publish a second node"),
        Err(error) => {
            let message = format!("{error:#}").to_lowercase();
            assert!(
                message.contains("conflict") || message.contains("name"),
                "unexpected import error: {error:#}"
            );
        }
    }
    // Ensure failure is durable even if the importer returns the error after marking failed.
    let failed = list_import_entries(
        &pool,
        DEFAULT_IMPORT_SOURCE_ID,
        Some(ImportEntryState::Failed),
    )
    .await
    .expect("list failed");
    let failure = failed.iter().find(|row| row.source_path == conflict_name);
    if let Some(failure) = failure {
        assert!(
            failure
                .error_message
                .as_deref()
                .is_some_and(|msg| !msg.is_empty()),
            "failure must carry a diagnostic message"
        );
    } else {
        // import_entry may propagate NameConflict after marking failed; re-check entry state.
        let entry = strife_db::list_import_entries(&pool, DEFAULT_IMPORT_SOURCE_ID, None)
            .await
            .expect("all entries")
            .into_iter()
            .find(|row| row.source_path == conflict_name)
            .expect("conflict entry still tracked");
        assert!(
            matches!(
                entry.state,
                ImportEntryState::Failed | ImportEntryState::Discovered
            ),
            "conflict entry state={:?}",
            entry.state
        );
    }
    assert!(
        watch_root.join(&conflict_name).exists(),
        "failed import leaves the source file in place"
    );
    assert_eq!(
        count_nodes(&pool, conflict_name.clone()).await,
        1,
        "exactly one active node for the conflicting name"
    );

    // Cleanup
    sqlx::query(
        r"
        DELETE FROM nodes
        WHERE parent_id = $1 AND name = ANY($2)
        ",
    )
    .bind(ROOT_NODE_ID)
    .bind(vec![file_name.clone(), conflict_name.clone()])
    .execute(&pool)
    .await
    .ok();
    sqlx::query("DELETE FROM import_entries WHERE source_id = $1 AND source_path LIKE $2")
        .bind(DEFAULT_IMPORT_SOURCE_ID)
        .bind(format!("%{fixture}%"))
        .execute(&pool)
        .await
        .ok();
    let _ = tokio::fs::remove_dir_all(&watch_root).await;
    let _ = tokio::fs::remove_dir_all(&storage_root).await;
}
