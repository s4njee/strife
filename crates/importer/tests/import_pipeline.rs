use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{DEFAULT_IMPORT_SOURCE_ID, MIGRATOR, ROOT_NODE_ID, list_pending_entries};
use strife_importer::{PostgresDiscoverySink, ScanOptions, import_entry, scan_directory};
use strife_storage::{DiskGuard, LocalFsBackend};
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
        let node = import_entry(
            &pool,
            &storage,
            &watch_root,
            ROOT_NODE_ID,
            entry,
            DiskGuard::new(100).expect("valid guard"),
        )
        .await
        .expect("import entry")
        .expect("stable entry finalized");
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
    assert_eq!(finalized_count, 5);

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
