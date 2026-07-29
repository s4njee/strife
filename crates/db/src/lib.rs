//! `PostgreSQL` access and migration support for Strife.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, migrate::Migrator};
use strife_domain::{FolderError, FolderRules, NodeId};
use uuid::Uuid;

/// Embedded, versioned database migrations.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Stable identifier for the single root folder created by migrations.
pub const ROOT_NODE_ID: Uuid = Uuid::from_u128(1);

/// Persisted node kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "node_kind", rename_all = "lowercase")]
pub enum NodeKind {
    Folder,
    File,
}

/// Persisted node lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "node_lifecycle_state", rename_all = "lowercase")]
pub enum LifecycleState {
    Active,
    Trashed,
    Deleted,
}

/// Persistence state for a managed file object.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "file_upload_state", rename_all = "lowercase")]
pub enum FileUploadState {
    Staging,
    Finalized,
}

/// Durable resumable-upload lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "upload_session_state", rename_all = "lowercase")]
pub enum UploadSessionState {
    Active,
    Finalizing,
    Completed,
    Cancelled,
    Expired,
}

/// Typed representation of a row in `nodes`.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct NodeRecord {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: NodeKind,
    pub lifecycle_state: LifecycleState,
    pub source_created_at: Option<DateTime<Utc>>,
    pub source_modified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Typed representation of a row in `file_objects`.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct FileObjectRecord {
    pub id: Uuid,
    pub node_id: Option<Uuid>,
    pub storage_key: String,
    pub byte_size: i64,
    pub mime_type: Option<String>,
    pub checksum_sha256: Option<String>,
    pub upload_state: FileUploadState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Typed representation of a row in `upload_sessions`.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct UploadSessionRecord {
    pub id: Uuid,
    pub target_folder_id: Uuid,
    pub display_name: String,
    pub expected_byte_size: Option<i64>,
    pub received_bytes: i64,
    pub staging_key: String,
    pub state: UploadSessionState,
    pub checksum_sha256: Option<String>,
    pub completed_node_id: Option<Uuid>,
    pub source_created_at: Option<DateTime<Utc>>,
    pub source_modified_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Parameters required to create a resumable upload session.
pub struct CreateUploadSession<'a> {
    pub target_folder_id: Uuid,
    pub display_name: &'a str,
    pub expected_byte_size: Option<i64>,
    pub staging_key: Uuid,
    pub source_created_at: Option<DateTime<Utc>>,
    pub source_modified_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
}

/// A persisted inclusive byte range.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct ReceivedRange {
    pub start_byte: i64,
    pub end_byte: i64,
}

/// A session and its ordered received ranges.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadSessionProgress {
    pub session: UploadSessionRecord,
    pub received_ranges: Vec<ReceivedRange>,
}

/// Expected errors while recording a resumable-upload range.
#[derive(Debug, thiserror::Error)]
pub enum RecordChunkError {
    #[error("upload session was not found")]
    NotFound,
    #[error("upload session is not active")]
    NotActive,
    #[error("byte range overlaps an existing chunk")]
    Overlap,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Expected failures while atomically finalizing an upload.
#[derive(Debug, thiserror::Error)]
pub enum FinalizeUploadError {
    #[error("upload session was not found")]
    NotFound,
    #[error("upload session is not active")]
    NotActive,
    #[error("upload session has incomplete bytes")]
    Incomplete,
    #[error("an active sibling already has this name")]
    NameConflict,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Minimal node information used for hierarchy paths.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct NodePathEntry {
    pub id: Uuid,
    pub name: String,
}

/// A folder that prevented an atomic batch move.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderMoveConflict {
    pub id: Uuid,
    pub name: String,
    pub reason: FolderMoveConflictReason,
}

/// Why a folder could not be moved to the requested destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FolderMoveConflictReason {
    NameConflict,
    CycleDetected,
}

/// Expected failures from a transactional folder mutation.
#[derive(Debug, thiserror::Error)]
pub enum FolderMutationError {
    #[error(transparent)]
    Rule(#[from] FolderError),
    #[error("one or more folders conflict with the requested destination")]
    MoveConflict(Vec<FolderMoveConflict>),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Confirms that a pool can execute a minimal query.
///
/// # Errors
///
/// Returns the database error when the query cannot be completed.
pub async fn ping(pool: &PgPool) -> Result<(), sqlx::Error> {
    let _: i32 = sqlx::query_scalar!(r#"SELECT 1 AS "value!""#)
        .fetch_one(pool)
        .await?;

    Ok(())
}

/// Creates a staged managed file object.
///
/// # Errors
///
/// Returns the database error when the object cannot be inserted.
pub async fn create_file_object(
    pool: &PgPool,
    storage_key: Uuid,
    byte_size: i64,
    mime_type: Option<&str>,
    checksum_sha256: Option<&str>,
) -> Result<FileObjectRecord, sqlx::Error> {
    sqlx::query_as::<_, FileObjectRecord>(
        r"
        INSERT INTO file_objects (
            id, storage_key, byte_size, mime_type, checksum_sha256, upload_state
        )
        VALUES ($1, $2, $3, $4, $5, 'staging')
        RETURNING
            id,
            node_id,
            storage_key,
            byte_size,
            mime_type,
            checksum_sha256,
            upload_state,
            created_at,
            updated_at
        ",
    )
    .bind(Uuid::new_v4())
    .bind(storage_key.simple().to_string())
    .bind(byte_size)
    .bind(mime_type)
    .bind(checksum_sha256)
    .fetch_one(pool)
    .await
}

/// Attaches a staged object to a node and marks it finalized.
///
/// # Errors
///
/// Returns `RowNotFound` for an unavailable staged object or the database error
/// when the node is invalid or already owns a finalized object.
pub async fn finalize_file_object(
    pool: &PgPool,
    file_object_id: Uuid,
    node_id: Uuid,
) -> Result<FileObjectRecord, sqlx::Error> {
    sqlx::query_as::<_, FileObjectRecord>(
        r"
        UPDATE file_objects
        SET node_id = $2, upload_state = 'finalized', updated_at = now()
        WHERE id = $1
          AND upload_state = 'staging'
        RETURNING
            id,
            node_id,
            storage_key,
            byte_size,
            mime_type,
            checksum_sha256,
            upload_state,
            created_at,
            updated_at
        ",
    )
    .bind(file_object_id)
    .bind(node_id)
    .fetch_optional(pool)
    .await?
    .ok_or(sqlx::Error::RowNotFound)
}

/// Fetches the finalized managed object attached to a node.
///
/// # Errors
///
/// Returns the database error when the query cannot be completed.
pub async fn get_file_object_by_node_id(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Option<FileObjectRecord>, sqlx::Error> {
    sqlx::query_as::<_, FileObjectRecord>(
        r"
        SELECT
            id,
            node_id,
            storage_key,
            byte_size,
            mime_type,
            checksum_sha256,
            upload_state,
            created_at,
            updated_at
        FROM file_objects
        WHERE node_id = $1
          AND upload_state = 'finalized'
        ",
    )
    .bind(node_id)
    .fetch_optional(pool)
    .await
}

/// Creates a durable active upload session.
///
/// # Errors
///
/// Returns the database error when the session cannot be inserted.
pub async fn create_session(
    pool: &PgPool,
    input: CreateUploadSession<'_>,
) -> Result<UploadSessionRecord, sqlx::Error> {
    upload_session_query(
        r"
        INSERT INTO upload_sessions (
            id,
            target_folder_id,
            display_name,
            expected_byte_size,
            staging_key,
            source_created_at,
            source_modified_at,
            expires_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING *
        ",
    )
    .bind(Uuid::new_v4())
    .bind(input.target_folder_id)
    .bind(input.display_name)
    .bind(input.expected_byte_size)
    .bind(input.staging_key.simple().to_string())
    .bind(input.source_created_at)
    .bind(input.source_modified_at)
    .bind(input.expires_at)
    .fetch_one(pool)
    .await
}

/// Atomically records a non-overlapping inclusive byte range.
///
/// # Errors
///
/// Returns `NotFound`, `NotActive`, or `Overlap` for expected session/range
/// failures and `Database` for unexpected database failures.
pub async fn record_chunk(
    pool: &PgPool,
    session_id: Uuid,
    start_byte: i64,
    end_byte: i64,
) -> Result<UploadSessionRecord, RecordChunkError> {
    let mut transaction = pool.begin().await?;
    let session = load_session_for_update(&mut transaction, session_id)
        .await?
        .ok_or(RecordChunkError::NotFound)?;
    if session.state != UploadSessionState::Active {
        return Err(RecordChunkError::NotActive);
    }

    let overlaps = sqlx::query_scalar::<_, bool>(
        r"
        SELECT EXISTS (
            SELECT 1
            FROM upload_chunks
            WHERE session_id = $1
              AND start_byte <= $3
              AND end_byte >= $2
        )
        ",
    )
    .bind(session_id)
    .bind(start_byte)
    .bind(end_byte)
    .fetch_one(&mut *transaction)
    .await?;
    if overlaps {
        return Err(RecordChunkError::Overlap);
    }

    sqlx::query(
        r"
        INSERT INTO upload_chunks (id, session_id, start_byte, end_byte)
        VALUES ($1, $2, $3, $4)
        ",
    )
    .bind(Uuid::new_v4())
    .bind(session_id)
    .bind(start_byte)
    .bind(end_byte)
    .execute(&mut *transaction)
    .await?;
    let byte_count = end_byte
        .checked_sub(start_byte)
        .and_then(|difference| difference.checked_add(1))
        .ok_or_else(|| sqlx::Error::Protocol("invalid upload chunk range".to_owned()))?;
    let updated = upload_session_query(
        r"
        UPDATE upload_sessions
        SET received_bytes = received_bytes + $2, updated_at = now()
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(session_id)
    .bind(byte_count)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(updated)
}

/// Loads a session with its ordered received byte ranges.
///
/// # Errors
///
/// Returns the database error when the session or ranges cannot be loaded.
pub async fn get_session_progress(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<Option<UploadSessionProgress>, sqlx::Error> {
    let Some(session) = get_upload_session(pool, session_id).await? else {
        return Ok(None);
    };
    let received_ranges = sqlx::query_as::<_, ReceivedRange>(
        r"
        SELECT start_byte, end_byte
        FROM upload_chunks
        WHERE session_id = $1
        ORDER BY start_byte
        ",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    Ok(Some(UploadSessionProgress {
        session,
        received_ranges,
    }))
}

/// Marks a session completed with its final checksum and optional file node.
///
/// # Errors
///
/// Returns `RowNotFound` for an unavailable session or a database error.
pub async fn finalize_session(
    pool: &PgPool,
    session_id: Uuid,
    checksum_sha256: &str,
    completed_node_id: Option<Uuid>,
) -> Result<UploadSessionRecord, sqlx::Error> {
    upload_session_query(
        r"
        UPDATE upload_sessions
        SET
            state = 'completed',
            checksum_sha256 = $2,
            completed_node_id = $3,
            updated_at = now()
        WHERE id = $1
          AND state IN ('active', 'finalizing', 'completed')
        RETURNING *
        ",
    )
    .bind(session_id)
    .bind(checksum_sha256)
    .bind(completed_node_id)
    .fetch_optional(pool)
    .await?
    .ok_or(sqlx::Error::RowNotFound)
}

/// Cancels an active/finalizing session and returns its stable state.
///
/// # Errors
///
/// Returns `RowNotFound` when the session does not exist or a database error.
pub async fn cancel_session(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<UploadSessionRecord, sqlx::Error> {
    let updated = upload_session_query(
        r"
        UPDATE upload_sessions
        SET state = 'cancelled', updated_at = now()
        WHERE id = $1
          AND state IN ('active', 'finalizing')
        RETURNING *
        ",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    match updated {
        Some(session) => Ok(session),
        None => get_upload_session(pool, session_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound),
    }
}

/// Marks an overdue active session expired and returns its stable state.
///
/// # Errors
///
/// Returns `RowNotFound` when the session does not exist or a database error.
pub async fn expire_session(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<UploadSessionRecord, sqlx::Error> {
    let updated = upload_session_query(
        r"
        UPDATE upload_sessions
        SET state = 'expired', updated_at = now()
        WHERE id = $1
          AND state = 'active'
          AND expires_at < now()
        RETURNING *
        ",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    match updated {
        Some(session) => Ok(session),
        None => get_upload_session(pool, session_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound),
    }
}

/// Lists active sessions whose expiry deadline has passed.
///
/// # Errors
///
/// Returns the database error when the query cannot be completed.
pub async fn list_expired_sessions(pool: &PgPool) -> Result<Vec<UploadSessionRecord>, sqlx::Error> {
    upload_session_query(
        r"
        SELECT *
        FROM upload_sessions
        WHERE state = 'active'
          AND expires_at < now()
        ORDER BY expires_at, id
        ",
    )
    .fetch_all(pool)
    .await
}

/// Lists active upload sessions targeting one folder in creation order.
///
/// # Errors
///
/// Returns the database error when the query cannot be completed.
pub async fn list_active_upload_sessions(
    pool: &PgPool,
    folder_id: Uuid,
) -> Result<Vec<UploadSessionRecord>, sqlx::Error> {
    upload_session_query(
        r"
        SELECT *
        FROM upload_sessions
        WHERE target_folder_id = $1
          AND state = 'active'
        ORDER BY created_at, id
        ",
    )
    .bind(folder_id)
    .fetch_all(pool)
    .await
}

/// Fetches any upload session by identifier.
///
/// # Errors
///
/// Returns the database error when the query cannot be completed.
pub async fn get_upload_session(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<Option<UploadSessionRecord>, sqlx::Error> {
    upload_session_query("SELECT * FROM upload_sessions WHERE id = $1")
        .bind(session_id)
        .fetch_optional(pool)
        .await
}

/// Atomically creates a file node/object, completes its upload session, and
/// enqueues metadata extraction. Repeated calls return the completed node.
///
/// # Errors
///
/// Returns an expected session/completeness/name error or an unexpected
/// database failure. The transaction commits all four records together.
pub async fn finalize_upload(
    pool: &PgPool,
    session_id: Uuid,
    original_storage_key: Uuid,
    byte_size: i64,
    mime_type: &str,
    checksum_sha256: &str,
) -> Result<NodeRecord, FinalizeUploadError> {
    let mut transaction = pool.begin().await?;
    let session = load_session_for_update(&mut transaction, session_id)
        .await?
        .ok_or(FinalizeUploadError::NotFound)?;
    if session.state == UploadSessionState::Completed {
        let node = load_completed_upload_node(&mut transaction, &session).await?;
        transaction.commit().await?;
        return Ok(node);
    }
    if session.state != UploadSessionState::Active {
        return Err(FinalizeUploadError::NotActive);
    }
    if session
        .expected_byte_size
        .is_some_and(|expected| expected != session.received_bytes)
    {
        return Err(FinalizeUploadError::Incomplete);
    }

    let node = insert_uploaded_node(&mut transaction, &session).await?;
    insert_finalized_object(
        &mut transaction,
        node.id,
        original_storage_key,
        byte_size,
        mime_type,
        checksum_sha256,
    )
    .await?;
    enqueue_metadata_job(&mut transaction, node.id).await?;
    complete_upload_session(&mut transaction, session_id, node.id, checksum_sha256).await?;
    transaction.commit().await?;
    Ok(node)
}

fn upload_session_query(
    sql: &'static str,
) -> sqlx::query::QueryAs<'static, sqlx::Postgres, UploadSessionRecord, sqlx::postgres::PgArguments>
{
    sqlx::query_as::<_, UploadSessionRecord>(sql)
}

async fn load_session_for_update(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
) -> Result<Option<UploadSessionRecord>, sqlx::Error> {
    upload_session_query("SELECT * FROM upload_sessions WHERE id = $1 FOR UPDATE")
        .bind(session_id)
        .fetch_optional(&mut **transaction)
        .await
}

async fn load_completed_upload_node(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session: &UploadSessionRecord,
) -> Result<NodeRecord, FinalizeUploadError> {
    let node_id = session
        .completed_node_id
        .ok_or_else(|| sqlx::Error::Protocol("completed upload has no node".to_owned()))?;
    load_node_in_transaction(transaction, node_id)
        .await?
        .ok_or(FinalizeUploadError::NotFound)
}

async fn insert_uploaded_node(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session: &UploadSessionRecord,
) -> Result<NodeRecord, FinalizeUploadError> {
    sqlx::query_as::<_, NodeRecord>(
        r"
        INSERT INTO nodes (
            id,
            parent_id,
            name,
            kind,
            source_created_at,
            source_modified_at
        )
        VALUES ($1, $2, $3, 'file', $4, $5)
        RETURNING
            id,
            parent_id,
            name,
            kind,
            lifecycle_state,
            source_created_at,
            source_modified_at,
            created_at,
            updated_at
        ",
    )
    .bind(Uuid::new_v4())
    .bind(session.target_folder_id)
    .bind(&session.display_name)
    .bind(session.source_created_at)
    .bind(session.source_modified_at)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_finalize_error)
}

async fn insert_finalized_object(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
    storage_key: Uuid,
    byte_size: i64,
    mime_type: &str,
    checksum_sha256: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r"
        INSERT INTO file_objects (
            id,
            node_id,
            storage_key,
            byte_size,
            mime_type,
            checksum_sha256,
            upload_state
        )
        VALUES ($1, $2, $3, $4, $5, $6, 'finalized')
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .bind(storage_key.simple().to_string())
    .bind(byte_size)
    .bind(mime_type)
    .bind(checksum_sha256)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn enqueue_metadata_job(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r"
        INSERT INTO jobs (id, job_type, target_node_id)
        VALUES ($1, 'metadata_extraction', $2)
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn complete_upload_session(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
    node_id: Uuid,
    checksum_sha256: &str,
) -> Result<(), sqlx::Error> {
    upload_session_query(
        r"
        UPDATE upload_sessions
        SET
            state = 'completed',
            checksum_sha256 = $2,
            completed_node_id = $3,
            updated_at = now()
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(session_id)
    .bind(checksum_sha256)
    .bind(node_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(())
}

async fn load_node_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
) -> Result<Option<NodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, NodeRecord>(
        r"
        SELECT
            id,
            parent_id,
            name,
            kind,
            lifecycle_state,
            source_created_at,
            source_modified_at,
            created_at,
            updated_at
        FROM nodes
        WHERE id = $1
        ",
    )
    .bind(node_id)
    .fetch_optional(&mut **transaction)
    .await
}

/// Fetches an active or inactive node by its identifier.
///
/// # Errors
///
/// Returns the database error when the query cannot be completed.
pub async fn get_node_by_id(pool: &PgPool, id: Uuid) -> Result<Option<NodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, NodeRecord>(
        r"
        SELECT
            id,
            parent_id,
            name,
            kind,
            lifecycle_state,
            source_created_at,
            source_modified_at,
            created_at,
            updated_at
        FROM nodes
        WHERE id = $1
        ",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Lists active children of a parent in case-sensitive name order.
///
/// # Errors
///
/// Returns the database error when the query cannot be completed.
pub async fn list_children(pool: &PgPool, parent_id: Uuid) -> Result<Vec<NodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, NodeRecord>(
        r"
        SELECT
            id,
            parent_id,
            name,
            kind,
            lifecycle_state,
            source_created_at,
            source_modified_at,
            created_at,
            updated_at
        FROM nodes
        WHERE parent_id = $1
          AND lifecycle_state = 'active'
        ORDER BY name, id
        ",
    )
    .bind(parent_id)
    .fetch_all(pool)
    .await
}

/// Checks for an active child with an exact display name.
///
/// # Errors
///
/// Returns the database error when the query cannot be completed.
pub async fn active_child_name_exists(
    pool: &PgPool,
    parent_id: Uuid,
    name: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>(
        r"
        SELECT EXISTS (
            SELECT 1
            FROM nodes
            WHERE parent_id = $1
              AND name = $2
              AND lifecycle_state = 'active'
        )
        ",
    )
    .bind(parent_id)
    .bind(name)
    .fetch_one(pool)
    .await
}

/// Lists one page of active children after an optional opaque node cursor.
///
/// # Errors
///
/// Returns the database error when the query cannot be completed.
pub async fn list_children_page(
    pool: &PgPool,
    parent_id: Uuid,
    cursor: Option<Uuid>,
    limit: u32,
) -> Result<Vec<NodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, NodeRecord>(
        r"
        SELECT
            id,
            parent_id,
            name,
            kind,
            lifecycle_state,
            source_created_at,
            source_modified_at,
            created_at,
            updated_at
        FROM nodes
        WHERE parent_id = $1
          AND lifecycle_state = 'active'
          AND (
              $2::uuid IS NULL
              OR (name, id) > (
                  SELECT name, id
                  FROM nodes
                  WHERE id = $2 AND parent_id = $1
              )
          )
        ORDER BY name, id
        LIMIT $3
        ",
    )
    .bind(parent_id)
    .bind(cursor)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
}

/// Returns an active folder path ordered from the root to the requested node.
///
/// # Errors
///
/// Returns the database error when the recursive query cannot be completed.
pub async fn list_ancestors(
    pool: &PgPool,
    folder_id: Uuid,
) -> Result<Vec<NodePathEntry>, sqlx::Error> {
    sqlx::query_as::<_, NodePathEntry>(
        r"
        WITH RECURSIVE ancestors AS (
            SELECT id, parent_id, name, 0 AS depth
            FROM nodes
            WHERE id = $1
              AND kind = 'folder'
              AND lifecycle_state = 'active'

            UNION ALL

            SELECT parent.id, parent.parent_id, parent.name, child.depth + 1
            FROM nodes AS parent
            JOIN ancestors AS child ON parent.id = child.parent_id
            WHERE parent.kind = 'folder'
              AND parent.lifecycle_state = 'active'
        )
        SELECT id, name
        FROM ancestors
        ORDER BY depth DESC
        ",
    )
    .bind(folder_id)
    .fetch_all(pool)
    .await
}

/// Creates an active folder below an existing active folder in one transaction.
///
/// # Errors
///
/// Returns `NotFound` for an invalid parent, `NameConflict` for duplicate active
/// sibling names, or `Database` for an unexpected database failure.
pub async fn create_folder(
    pool: &PgPool,
    parent_id: Uuid,
    name: &str,
) -> Result<NodeRecord, FolderMutationError> {
    FolderRules::validate_name(name)?;
    let mut transaction = pool.begin().await?;
    ensure_active_folder(&mut transaction, parent_id).await?;

    let result = sqlx::query_as::<_, NodeRecord>(
        r"
        INSERT INTO nodes (id, parent_id, name, kind)
        VALUES ($1, $2, $3, 'folder')
        RETURNING
            id,
            parent_id,
            name,
            kind,
            lifecycle_state,
            source_created_at,
            source_modified_at,
            created_at,
            updated_at
        ",
    )
    .bind(Uuid::new_v4())
    .bind(parent_id)
    .bind(name)
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_mutation_error)?;

    transaction.commit().await?;
    Ok(result)
}

/// Renames and/or moves an active folder in one transaction.
///
/// # Errors
///
/// Returns an expected mutation error for missing folders, name conflicts, and
/// cycles, or `Database` for an unexpected database failure.
pub async fn update_folder(
    pool: &PgPool,
    folder_id: Uuid,
    name: Option<&str>,
    parent_id: Option<Uuid>,
) -> Result<NodeRecord, FolderMutationError> {
    if let Some(name) = name {
        FolderRules::validate_name(name)?;
    }
    let mut transaction = pool.begin().await?;
    ensure_active_folder(&mut transaction, folder_id).await?;

    if let Some(destination_id) = parent_id {
        ensure_active_folder(&mut transaction, destination_id).await?;
        let descendant_ids = descendant_ids(&mut transaction, folder_id).await?;
        FolderRules::validate_move_target(
            NodeId::new(folder_id),
            NodeId::new(destination_id),
            &descendant_ids,
        )?;
    }

    let result = sqlx::query_as::<_, NodeRecord>(
        r"
        UPDATE nodes
        SET
            name = COALESCE($2, name),
            parent_id = COALESCE($3, parent_id),
            updated_at = now()
        WHERE id = $1
          AND kind = 'folder'
          AND lifecycle_state = 'active'
        RETURNING
            id,
            parent_id,
            name,
            kind,
            lifecycle_state,
            source_created_at,
            source_modified_at,
            created_at,
            updated_at
        ",
    )
    .bind(folder_id)
    .bind(name)
    .bind(parent_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(map_mutation_error)?
    .ok_or(FolderError::NotFound)?;

    transaction.commit().await?;
    Ok(result)
}

/// Moves active folders to one destination in a single transaction.
///
/// # Errors
///
/// Returns `NotFound` if a source or destination is unavailable, `MoveConflict`
/// with the affected folders for name/cycle conflicts, or `Database` for an
/// unexpected database failure. No folder is moved when any validation fails.
pub async fn move_folders(
    pool: &PgPool,
    folder_ids: &[Uuid],
    destination_id: Uuid,
) -> Result<Vec<NodeRecord>, FolderMutationError> {
    let mut ids = folder_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();

    let mut transaction = pool.begin().await?;
    ensure_active_folder(&mut transaction, destination_id).await?;
    let folders = load_move_folders(&mut transaction, &ids).await?;
    if folders.len() != ids.len() {
        return Err(FolderError::NotFound.into());
    }

    let conflicts = find_move_conflicts(&mut transaction, &folders, &ids, destination_id).await?;
    if !conflicts.is_empty() {
        return Err(FolderMutationError::MoveConflict(conflicts));
    }

    let moved = execute_folder_move(&mut transaction, &folders, &ids, destination_id).await?;
    transaction.commit().await?;
    Ok(moved)
}

async fn load_move_folders(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ids: &[Uuid],
) -> Result<Vec<NodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, NodeRecord>(
        r"
        SELECT
            id,
            parent_id,
            name,
            kind,
            lifecycle_state,
            source_created_at,
            source_modified_at,
            created_at,
            updated_at
        FROM nodes
        WHERE id = ANY($1)
          AND kind = 'folder'
          AND lifecycle_state = 'active'
        ORDER BY name, id
        FOR UPDATE
        ",
    )
    .bind(ids)
    .fetch_all(&mut **transaction)
    .await
}

async fn find_move_conflicts(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    folders: &[NodeRecord],
    ids: &[Uuid],
    destination_id: Uuid,
) -> Result<Vec<FolderMoveConflict>, sqlx::Error> {
    let mut conflicts = Vec::new();
    for folder in folders {
        let descendants = descendant_ids(transaction, folder.id).await?;
        if FolderRules::validate_move_target(
            NodeId::new(folder.id),
            NodeId::new(destination_id),
            &descendants,
        )
        .is_err()
        {
            conflicts.push(FolderMoveConflict {
                id: folder.id,
                name: folder.name.clone(),
                reason: FolderMoveConflictReason::CycleDetected,
            });
        }
    }

    for folder in folders {
        let duplicate_count = folders
            .iter()
            .filter(|candidate| candidate.name == folder.name)
            .count();
        let destination_conflict = sqlx::query_scalar::<_, bool>(
            r"
            SELECT EXISTS (
                SELECT 1
                FROM nodes
                WHERE parent_id = $1
                  AND name = $2
                  AND lifecycle_state = 'active'
                  AND NOT (id = ANY($3))
            )
            ",
        )
        .bind(destination_id)
        .bind(&folder.name)
        .bind(ids)
        .fetch_one(&mut **transaction)
        .await?;
        if (duplicate_count > 1 || destination_conflict)
            && !conflicts.iter().any(|conflict| conflict.id == folder.id)
        {
            conflicts.push(FolderMoveConflict {
                id: folder.id,
                name: folder.name.clone(),
                reason: FolderMoveConflictReason::NameConflict,
            });
        }
    }

    Ok(conflicts)
}

async fn execute_folder_move(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    folders: &[NodeRecord],
    ids: &[Uuid],
    destination_id: Uuid,
) -> Result<Vec<NodeRecord>, FolderMutationError> {
    sqlx::query_as::<_, NodeRecord>(
        r"
        UPDATE nodes
        SET parent_id = $2, updated_at = now()
        WHERE id = ANY($1)
          AND kind = 'folder'
          AND lifecycle_state = 'active'
        RETURNING
            id,
            parent_id,
            name,
            kind,
            lifecycle_state,
            source_created_at,
            source_modified_at,
            created_at,
            updated_at
        ",
    )
    .bind(ids)
    .bind(destination_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| {
        if is_unique_violation(&error) {
            FolderMutationError::MoveConflict(
                folders
                    .iter()
                    .map(|folder| FolderMoveConflict {
                        id: folder.id,
                        name: folder.name.clone(),
                        reason: FolderMoveConflictReason::NameConflict,
                    })
                    .collect(),
            )
        } else {
            FolderMutationError::Database(error)
        }
    })
}

async fn ensure_active_folder(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    folder_id: Uuid,
) -> Result<(), FolderMutationError> {
    let exists = sqlx::query_scalar::<_, bool>(
        r"
        SELECT EXISTS (
            SELECT 1
            FROM nodes
            WHERE id = $1
              AND kind = 'folder'
              AND lifecycle_state = 'active'
        )
        ",
    )
    .bind(folder_id)
    .fetch_one(&mut **transaction)
    .await?;

    if exists {
        Ok(())
    } else {
        Err(FolderError::NotFound.into())
    }
}

async fn descendant_ids(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    folder_id: Uuid,
) -> Result<Vec<NodeId>, sqlx::Error> {
    let ids = sqlx::query_scalar::<_, Uuid>(
        r"
        WITH RECURSIVE descendants AS (
            SELECT id
            FROM nodes
            WHERE parent_id = $1
              AND lifecycle_state = 'active'

            UNION ALL

            SELECT child.id
            FROM nodes AS child
            JOIN descendants AS parent ON child.parent_id = parent.id
            WHERE child.lifecycle_state = 'active'
        )
        SELECT id FROM descendants
        ",
    )
    .bind(folder_id)
    .fetch_all(&mut **transaction)
    .await?;

    Ok(ids.into_iter().map(NodeId::new).collect())
}

fn map_mutation_error(error: sqlx::Error) -> FolderMutationError {
    if is_unique_violation(&error) {
        return FolderError::NameConflict.into();
    }

    FolderMutationError::Database(error)
}

fn map_finalize_error(error: sqlx::Error) -> FinalizeUploadError {
    if is_unique_violation(&error) {
        FinalizeUploadError::NameConflict
    } else {
        FinalizeUploadError::Database(error)
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database_error) if database_error.code().as_deref() == Some("23505"))
}
