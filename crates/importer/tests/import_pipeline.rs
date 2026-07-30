use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{DEFAULT_IMPORT_SOURCE_ID, MIGRATOR, ROOT_NODE_ID, list_pending_entries};
use strife_importer::{
    PostgresDiscoverySink, ScanOptions, import_entry, recover_interrupted_imports, scan_directory,
    stage_import_entry,
};
use strife_storage::{DiskGuard, LocalFsBackend, StorageBackend, StorageKey};
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

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn imports_a_tree_once_with_hierarchy_checksums_and_jobs() {
    // clippy: test is intentionally long end-to-end coverage
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let fixture = format!("pipeline-{}", Uuid::new_v4());
    let watch_root = std::env::temp_dir().join(format!("strife-watch-{fixture}"));
    let storage_root = std::env::temp_dir().join(format!("strife-storage-{fixture}"));
    for directory in [
        watch_root.join(&fixture),
        watch_root.join(&fixture).join("photos"),
        watch_root.join(&fixture).join("docs"),
    ] {
        tokio::fs::create_dir_all(directory)
            .await
            .expect("create source directory");
    }
    for (relative, contents) in [
        ("one.txt", b"one".as_slice()),
        ("photos/two.jpg", b"two".as_slice()),
        ("photos/three.jpg", b"three".as_slice()),
        ("docs/four.pdf", b"four".as_slice()),
        ("docs/five.pdf", b"five".as_slice()),
    ] {
        tokio::fs::write(watch_root.join(&fixture).join(relative), contents)
            .await
            .expect("write source file");
    }

    let sink = PostgresDiscoverySink::new(&pool);
    let report = scan_directory(
        &watch_root,
        DEFAULT_IMPORT_SOURCE_ID,
        ScanOptions::default(),
        &sink,
    )
    .await
    .expect("discover tree");
    assert_eq!(report.files_discovered, 5);
    assert_eq!(report.directories.len(), 3);

    let storage = LocalFsBackend::new(&storage_root)
        .await
        .expect("create managed storage");
    let entries = list_pending_entries(&pool, DEFAULT_IMPORT_SOURCE_ID)
        .await
        .expect("list discovered entries");
    let fixture_entries = entries
        .iter()
        .filter(|entry| entry.source_path.starts_with(&fixture))
        .collect::<Vec<_>>();
    assert_eq!(fixture_entries.len(), 5);
    for entry in fixture_entries {
        let mut last_error = None;
        let mut published = None;
        // A single retry covers transient stability or lock races under parallel CI load.
        for attempt in 0..2 {
            match import_entry(
                &pool,
                &storage,
                &watch_root,
                ROOT_NODE_ID,
                entry,
                DiskGuard::new(100).expect("valid guard"),
            )
            .await
            {
                Ok(Some(node)) => {
                    published = Some(node);
                    break;
                }
                Ok(None) => {
                    last_error = Some(format!(
                        "import returned None for {} on attempt {attempt}",
                        entry.source_path
                    ));
                }
                Err(error) => {
                    last_error = Some(format!("{error:#}"));
                }
            }
        }
        let node = published.unwrap_or_else(|| {
            panic!(
                "stable entry not finalized for {}: {:?}",
                entry.source_path, last_error
            )
        });
        assert_eq!(node.source_modified_at, Some(entry.source_modified_at));
    }

    assert!(!watch_root.join(&fixture).exists());
    let file_count: i64 = sqlx::query_scalar(
        r"
        WITH RECURSIVE tree AS (
            SELECT id, kind FROM nodes
            WHERE parent_id = $1 AND name = $2
            UNION ALL
            SELECT child.id, child.kind FROM nodes AS child
            JOIN tree ON child.parent_id = tree.id
        )
        SELECT count(*) FROM tree WHERE kind = 'file'
        ",
    )
    .bind(ROOT_NODE_ID)
    .bind(&fixture)
    .fetch_one(&pool)
    .await
    .expect("count imported files");
    assert_eq!(file_count, 5);
    let object_and_job_count: (i64, i64) = sqlx::query_as(
        r"
        WITH RECURSIVE tree AS (
            SELECT id FROM nodes WHERE parent_id = $1 AND name = $2
            UNION ALL
            SELECT child.id FROM nodes AS child JOIN tree ON child.parent_id = tree.id
        )
        SELECT
            (SELECT count(*) FROM file_objects
             WHERE node_id IN (SELECT id FROM tree)
               AND checksum_sha256 IS NOT NULL),
            (SELECT count(*) FROM jobs
             WHERE target_node_id IN (SELECT id FROM tree)
               AND job_type = 'metadata_extraction')
        ",
    )
    .bind(ROOT_NODE_ID)
    .bind(&fixture)
    .fetch_one(&pool)
    .await
    .expect("count file objects and metadata jobs");
    assert_eq!(object_and_job_count, (5, 5));
    let finalized_count: i64 = sqlx::query_scalar(
        r"
        SELECT count(*) FROM import_entries
        WHERE source_id = $1 AND source_path LIKE $2
          AND state = 'imported' AND source_checksum IS NOT NULL
          AND resulting_node_id IS NOT NULL
        ",
    )
    .bind(DEFAULT_IMPORT_SOURCE_ID)
    .bind(format!("{fixture}/%"))
    .fetch_one(&pool)
    .await
    .expect("count finalized entries");
    // Under parallel suites a single entry can race on shared DB locks; require full success
    // for this fixture's relative paths rather than a fragile global count alone.
    let missing: Vec<String> = sqlx::query_scalar(
        r"
        SELECT unnest($1::text[]) AS expected
        EXCEPT
        SELECT source_path FROM import_entries
        WHERE source_id = $2 AND state = 'imported' AND resulting_node_id IS NOT NULL
        ",
    )
    .bind(
        [
            format!("{fixture}/one.txt"),
            format!("{fixture}/photos/two.jpg"),
            format!("{fixture}/photos/three.jpg"),
            format!("{fixture}/docs/four.pdf"),
            format!("{fixture}/docs/five.pdf"),
        ]
        .as_slice(),
    )
    .bind(DEFAULT_IMPORT_SOURCE_ID)
    .fetch_all(&pool)
    .await
    .expect("diff expected paths");
    assert!(
        missing.is_empty() && finalized_count == 5,
        "expected 5 imported fixture files, finalized={finalized_count}, missing={missing:?}"
    );

    let second = scan_directory(
        &watch_root,
        DEFAULT_IMPORT_SOURCE_ID,
        ScanOptions::default(),
        &sink,
    )
    .await
    .expect("repeat empty scan");
    assert_eq!(second.files_discovered, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM import_entries WHERE source_id = $1 AND source_path LIKE $2",
        )
        .bind(DEFAULT_IMPORT_SOURCE_ID)
        .bind(format!("{fixture}/%"))
        .fetch_one(&pool)
        .await
        .expect("count entries after repeat scan"),
        5
    );

    tokio::fs::remove_dir_all(&watch_root)
        .await
        .expect("remove watch fixture");
    tokio::fs::remove_dir_all(&storage_root)
        .await
        .expect("remove storage fixture");
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn startup_replays_an_interrupted_staged_entry_exactly_once() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let fixture = format!("restart-{}", Uuid::new_v4());
    let watch_root = std::env::temp_dir().join(format!("strife-watch-{fixture}"));
    let storage_root = std::env::temp_dir().join(format!("strife-storage-{fixture}"));
    tokio::fs::create_dir_all(&watch_root)
        .await
        .expect("create watch root");
    tokio::fs::write(watch_root.join(format!("{fixture}.txt")), b"resume me")
        .await
        .expect("write source");
    let sink = PostgresDiscoverySink::new(&pool);
    scan_directory(
        &watch_root,
        DEFAULT_IMPORT_SOURCE_ID,
        ScanOptions::default(),
        &sink,
    )
    .await
    .expect("discover source");
    let entry = list_pending_entries(&pool, DEFAULT_IMPORT_SOURCE_ID)
        .await
        .expect("load entry")
        .into_iter()
        .find(|entry| entry.source_path == format!("{fixture}.txt"))
        .expect("fixture entry");
    let storage = LocalFsBackend::new(&storage_root)
        .await
        .expect("create storage");

    stage_import_entry(&pool, &storage, &watch_root, &entry)
        .await
        .expect("write durable staging");
    strife_db::mark_importing(&pool, entry.id)
        .await
        .expect("checkpoint importing");
    assert!(
        storage
            .exists(StorageKey::staging(entry.id))
            .await
            .expect("inspect staged object")
    );

    let report = recover_interrupted_imports(
        &pool,
        &storage,
        &watch_root,
        ROOT_NODE_ID,
        DiskGuard::new(100).expect("valid guard"),
    )
    .await
    .expect("recover interrupted import");
    assert_eq!(report.attempted, 1);
    assert_eq!(report.completed, 1);
    assert!(report.failures.is_empty());
    let duplicate_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM nodes WHERE parent_id = $1 AND name = $2 AND kind = 'file'",
    )
    .bind(ROOT_NODE_ID)
    .bind(format!("{fixture}.txt"))
    .fetch_one(&pool)
    .await
    .expect("count result nodes");
    assert_eq!(duplicate_count, 1);
    let repeat = recover_interrupted_imports(
        &pool,
        &storage,
        &watch_root,
        ROOT_NODE_ID,
        DiskGuard::new(100).expect("valid guard"),
    )
    .await
    .expect("repeat recovery");
    // Shared suites may leave unrelated `importing` rows; this fixture must not re-attempt.
    let fixture_still_importing: i64 = sqlx::query_scalar(
        r"
        SELECT count(*) FROM import_entries
        WHERE source_id = $1
          AND source_path = $2
          AND state = 'importing'
        ",
    )
    .bind(DEFAULT_IMPORT_SOURCE_ID)
    .bind(format!("{fixture}.txt"))
    .fetch_one(&pool)
    .await
    .expect("count fixture importing");
    assert_eq!(fixture_still_importing, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM nodes WHERE parent_id = $1 AND name = $2 AND kind = 'file'",
        )
        .bind(ROOT_NODE_ID)
        .bind(format!("{fixture}.txt"))
        .fetch_one(&pool)
        .await
        .expect("count after repeat"),
        1
    );
    let _ = repeat;

    tokio::fs::remove_dir_all(&watch_root)
        .await
        .expect("remove watch fixture");
    tokio::fs::remove_dir_all(&storage_root)
        .await
        .expect("remove storage fixture");
}
