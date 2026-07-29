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

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database_error) if database_error.code().as_deref() == Some("23505"))
}
