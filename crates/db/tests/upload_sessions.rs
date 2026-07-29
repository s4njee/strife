use chrono::{Duration, Utc};
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    CreateUploadSession, MIGRATOR, ROOT_NODE_ID, RecordChunkError, UploadSessionState,
    cancel_session, create_session, finalize_session, get_session_progress, list_expired_sessions,
    record_chunk,
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

async fn create_fixture_folder(pool: &PgPool) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'folder')")
        .bind(id)
        .bind(ROOT_NODE_ID)
        .bind(format!("upload-session-test-{id}"))
        .execute(pool)
        .await
        .expect("create fixture folder");
    id
}

fn session_input(
    folder_id: Uuid,
    name: &str,
    expires_at: chrono::DateTime<Utc>,
) -> CreateUploadSession<'_> {
    CreateUploadSession {
        target_folder_id: folder_id,
        display_name: name,
        expected_byte_size: Some(10),
        staging_key: Uuid::new_v4(),
        source_created_at: None,
        source_modified_at: None,
        expires_at,
    }
}

#[tokio::test]
async fn sessions_track_ranges_lifecycle_and_expiration() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let folder_id = create_fixture_folder(&pool).await;
    let session = create_session(
        &pool,
        session_input(folder_id, "resume.bin", Utc::now() + Duration::hours(1)),
    )
    .await
    .expect("create session");
    assert_eq!(session.state, UploadSessionState::Active);

    record_chunk(&pool, session.id, 5, 9)
        .await
        .expect("record later chunk");
    record_chunk(&pool, session.id, 0, 4)
        .await
        .expect("record earlier chunk");
    assert!(matches!(
        record_chunk(&pool, session.id, 3, 6).await,
        Err(RecordChunkError::Overlap)
    ));
    let progress = get_session_progress(&pool, session.id)
        .await
        .expect("load progress")
        .expect("session exists");
    assert_eq!(progress.session.received_bytes, 10);
    assert_eq!(progress.received_ranges[0].start_byte, 0);
    assert_eq!(progress.received_ranges[1].start_byte, 5);

    let completed = finalize_session(&pool, session.id, "checksum", None)
        .await
        .expect("complete session");
    assert_eq!(completed.state, UploadSessionState::Completed);

    let cancellable = create_session(
        &pool,
        session_input(folder_id, "cancel.bin", Utc::now() + Duration::hours(1)),
    )
    .await
    .expect("create cancellable session");
    assert_eq!(
        cancel_session(&pool, cancellable.id)
            .await
            .expect("cancel session")
            .state,
        UploadSessionState::Cancelled
    );
    assert_eq!(
        cancel_session(&pool, cancellable.id)
            .await
            .expect("repeat cancellation")
            .state,
        UploadSessionState::Cancelled
    );

    let expired = create_session(
        &pool,
        session_input(folder_id, "expired.bin", Utc::now() - Duration::minutes(1)),
    )
    .await
    .expect("create expired session");
    assert!(
        list_expired_sessions(&pool)
            .await
            .expect("list expired sessions")
            .iter()
            .any(|session| session.id == expired.id)
    );

    sqlx::query("DELETE FROM upload_sessions WHERE target_folder_id = $1")
        .bind(folder_id)
        .execute(&pool)
        .await
        .expect("remove sessions");
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(folder_id)
        .execute(&pool)
        .await
        .expect("remove fixture folder");
}
