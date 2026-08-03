//! `PostgreSQL` access and migration support for Strife.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, migrate::Migrator};
use strife_domain::{FolderError, FolderRules, NodeId};
use uuid::Uuid;

/// Embedded, versioned database migrations.
/// Embedded schema migrations, including the live-queue indexes in migration 28.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Stable identifier for the single root folder created by migrations.
pub const ROOT_NODE_ID: Uuid = Uuid::from_u128(1);

/// Stable identifier for the single v1 watched-folder source.
pub const DEFAULT_IMPORT_SOURCE_ID: Uuid = Uuid::from_u128(3);

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

/// Durable lifecycle of a file discovered in the import inbox.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "import_entry_state", rename_all = "lowercase")]
pub enum ImportEntryState {
    Discovered,
    Stable,
    Importing,
    Imported,
    Failed,
}

/// Provenance of text persisted for a document.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "document_text_source", rename_all = "lowercase")]
pub enum DocumentTextSource {
    Embedded,
    Ocr,
}

/// Durable extraction state for a document's text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "document_text_status", rename_all = "lowercase")]
pub enum DocumentTextStatus {
    Pending,
    Completed,
    Failed,
    Skipped,
    Unsupported,
}

/// One document-level text extraction record.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct DocumentTextRecord {
    pub node_id: Uuid,
    pub source: DocumentTextSource,
    pub status: DocumentTextStatus,
    pub language: String,
    pub engine_name: String,
    pub engine_version: String,
    pub page_count: Option<i32>,
    pub mean_confidence: Option<f32>,
    pub char_count: i32,
    pub warnings: Vec<String>,
    pub duration_ms: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One page of embedded or OCR-produced text.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct DocumentTextPageRecord {
    pub id: Uuid,
    pub node_id: Uuid,
    pub page_number: i32,
    pub content: String,
    pub confidence: Option<f32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

/// Document-level values persisted by an extraction attempt.
pub struct UpsertDocumentText<'a> {
    pub node_id: Uuid,
    pub source: DocumentTextSource,
    pub status: DocumentTextStatus,
    pub language: &'a str,
    pub engine_name: &'a str,
    pub engine_version: &'a str,
    pub page_count: Option<i32>,
    pub mean_confidence: Option<f32>,
    pub char_count: i32,
    pub warnings: &'a [String],
    pub duration_ms: Option<i64>,
}

/// Page values used when atomically replacing a document's text.
pub struct DocumentTextPageInput<'a> {
    pub page_number: i32,
    pub content: &'a str,
    pub confidence: Option<f32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

/// Number of document text records in one durable state.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct DocumentTextStatusCount {
    pub status: DocumentTextStatus,
    pub count: i64,
}

/// OCR engine identity reported by the active worker.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct OcrEngineState {
    pub engine_name: String,
    pub engine_version: String,
    pub language: String,
    pub updated_at: DateTime<Utc>,
}

/// Durable per-file OCR activity used by the event stream.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct OcrEventRecord {
    pub id: i64,
    pub node_id: Option<Uuid>,
    pub node_name: String,
    pub state: String,
    pub page_count: Option<i32>,
    pub mean_confidence: Option<f32>,
    pub warning: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// One ranked page-level document text match.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct DocumentTextSearchResult {
    pub cursor_id: Uuid,
    pub node_id: Uuid,
    pub node_name: String,
    pub page_number: i32,
    pub snippet: String,
    pub score: f32,
}

/// Indexed aggregate OCR workload and durable outcome counts.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct OcrStatusCounts {
    pub pending: i64,
    pub running: i64,
    pub completed: i64,
    pub failed: i64,
    pub skipped: i64,
    pub unsupported: i64,
    pub remaining: i64,
}

/// Durable metadata job activity used by the operator console event stream.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct MetadataEventRecord {
    pub id: i64,
    pub job_id: Option<Uuid>,
    pub node_id: Option<Uuid>,
    pub node_name: String,
    pub state: String,
    pub attempt: i32,
    pub extractor_name: Option<String>,
    pub duration_ms: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Indexed aggregate state for metadata extraction jobs.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct MetadataStatusCounts {
    pub pending: i64,
    pub running: i64,
    pub completed: i64,
    pub failed: i64,
    pub skipped: i64,
    pub cancelled: i64,
    pub remaining: i64,
    pub total: i64,
    pub completed_per_hour: i64,
}

mod grouping;

pub use grouping::{
    EmailDuplicateReason, EmailGrouping, EmailThreadReason, GroupingEvidence, group_email,
};

/// Kind of durable background work.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "job_type", rename_all = "snake_case")]
pub enum JobType {
    MetadataExtraction,
    PreviewGeneration,
    TrashCleanup,
    PermanentDeletion,
    ImportScan,
    Ocr,
    EmailExtraction,
    AttachmentExtraction,
}

/// Lifecycle state of a durable background job.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "job_state", rename_all = "lowercase")]
pub enum JobState {
    Pending,
    Leased,
    Completed,
    Failed,
    Cancelled,
    Skipped,
}

/// Why a job entered the queue. Claiming always favors interactive work.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, sqlx::Type)]
#[sqlx(type_name = "job_origin", rename_all = "snake_case")]
pub enum JobOrigin {
    Foreground,
    Repair,
    Backfill,
}

/// Shared capacity pool consumed by a job while it is leased.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, sqlx::Type)]
#[sqlx(type_name = "job_resource_class", rename_all = "snake_case")]
pub enum JobResourceClass {
    Light,
    Extractor,
    Preview,
    HeavyCpu,
    HeavyIo,
}

/// A bounded historical enrichment workload.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, sqlx::Type)]
#[sqlx(type_name = "backfill_kind", rename_all = "snake_case")]
pub enum BackfillKind {
    Email,
    Ocr,
    AttachmentText,
    AttachmentOcr,
}

/// Durable operator-controlled lifecycle for a backfill campaign.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, sqlx::Type)]
#[sqlx(type_name = "backfill_state", rename_all = "snake_case")]
pub enum BackfillState {
    Draft,
    Paused,
    Running,
    Draining,
    Completed,
    Cancelled,
    Failed,
}

/// Persisted kind of an audio-visual stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "media_stream_type", rename_all = "lowercase")]
pub enum MediaStreamType {
    Video,
    Audio,
    Subtitle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "artifact_type", rename_all = "lowercase")]
pub enum ArtifactType {
    Thumbnail,
    Preview,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "artifact_state", rename_all = "lowercase")]
pub enum ArtifactState {
    Generating,
    Ready,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct DerivedArtifactRecord {
    pub id: Uuid,
    pub node_id: Uuid,
    pub artifact_type: ArtifactType,
    pub format: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub storage_key: String,
    pub byte_size: i64,
    pub generator_version: String,
    pub state: ArtifactState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct UpsertArtifact<'a> {
    pub node_id: Uuid,
    pub artifact_type: ArtifactType,
    pub format: &'a str,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub storage_key: &'a str,
    pub byte_size: i64,
    pub generator_version: &'a str,
    pub state: ArtifactState,
}

/// Fetches one cached artifact.
///
/// # Errors
/// Returns a database error when the artifact cannot be queried.
pub async fn get_artifact(
    pool: &PgPool,
    node_id: Uuid,
    artifact_type: ArtifactType,
) -> Result<Option<DerivedArtifactRecord>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM derived_artifacts WHERE node_id = $1 AND artifact_type = $2")
        .bind(node_id)
        .bind(artifact_type)
        .fetch_optional(pool)
        .await
}

/// Creates or replaces the durable state for one artifact type.
///
/// # Errors
/// Returns a database error when the artifact cannot be persisted.
pub async fn create_or_update_artifact(
    pool: &PgPool,
    input: &UpsertArtifact<'_>,
) -> Result<DerivedArtifactRecord, sqlx::Error> {
    sqlx::query_as(r"INSERT INTO derived_artifacts (id,node_id,artifact_type,format,width,height,storage_key,byte_size,generator_version,state)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        ON CONFLICT (node_id,artifact_type) DO UPDATE SET format=EXCLUDED.format,width=EXCLUDED.width,height=EXCLUDED.height,storage_key=EXCLUDED.storage_key,byte_size=EXCLUDED.byte_size,generator_version=EXCLUDED.generator_version,state=EXCLUDED.state,updated_at=now()
        RETURNING *")
        .bind(Uuid::new_v4()).bind(input.node_id).bind(input.artifact_type).bind(input.format)
        .bind(input.width).bind(input.height).bind(input.storage_key).bind(input.byte_size)
        .bind(input.generator_version).bind(input.state).fetch_one(pool).await
}

/// Normalized media stream values written after a successful probe.
pub struct MediaStreamInput<'a> {
    pub stream_index: i32,
    pub stream_type: MediaStreamType,
    pub codec: &'a str,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration_ms: Option<i64>,
    pub bitrate_bps: Option<i64>,
    pub frame_rate: Option<&'a str>,
    pub language: Option<&'a str>,
}

/// Atomically replaces the normalized streams for a node after a successful probe.
///
/// # Errors
///
/// Returns a database error and rolls back when existing rows cannot be replaced completely.
pub async fn replace_media_streams(
    pool: &PgPool,
    node_id: Uuid,
    streams: &[MediaStreamInput<'_>],
) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("DELETE FROM media_streams WHERE node_id = $1")
        .bind(node_id)
        .execute(&mut *transaction)
        .await?;
    for stream in streams {
        sqlx::query(
            r"
            INSERT INTO media_streams (
                id, node_id, stream_index, stream_type, codec, width, height,
                duration_ms, bitrate_bps, frame_rate, language
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ",
        )
        .bind(Uuid::new_v4())
        .bind(node_id)
        .bind(stream.stream_index)
        .bind(stream.stream_type)
        .bind(stream.codec)
        .bind(stream.width)
        .bind(stream.height)
        .bind(stream.duration_ms)
        .bind(stream.bitrate_bps)
        .bind(stream.frame_rate)
        .bind(stream.language)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await
}

/// Creates or replaces the document-level state for one node.
///
/// # Errors
///
/// Returns a database error when the document text record cannot be persisted.
pub async fn upsert_document_text(
    pool: &PgPool,
    input: &UpsertDocumentText<'_>,
) -> Result<DocumentTextRecord, sqlx::Error> {
    sqlx::query_as::<_, DocumentTextRecord>(
        r"
        INSERT INTO document_text (
            node_id, source, status, language, engine_name, engine_version,
            page_count, mean_confidence, char_count, warnings, duration_ms
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (node_id) DO UPDATE SET
            source = EXCLUDED.source,
            status = EXCLUDED.status,
            language = EXCLUDED.language,
            engine_name = EXCLUDED.engine_name,
            engine_version = EXCLUDED.engine_version,
            page_count = EXCLUDED.page_count,
            mean_confidence = EXCLUDED.mean_confidence,
            char_count = EXCLUDED.char_count,
            warnings = EXCLUDED.warnings,
            duration_ms = EXCLUDED.duration_ms,
            updated_at = now()
        RETURNING *
        ",
    )
    .bind(input.node_id)
    .bind(input.source)
    .bind(input.status)
    .bind(input.language)
    .bind(input.engine_name)
    .bind(input.engine_version)
    .bind(input.page_count)
    .bind(input.mean_confidence)
    .bind(input.char_count)
    .bind(input.warnings)
    .bind(input.duration_ms)
    .fetch_one(pool)
    .await
}

/// Atomically replaces every stored page for one document.
///
/// # Errors
///
/// Returns a database error and rolls back when the complete replacement
/// cannot be committed.
pub async fn replace_document_text_pages(
    pool: &PgPool,
    node_id: Uuid,
    pages: &[DocumentTextPageInput<'_>],
) -> Result<Vec<DocumentTextPageRecord>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("DELETE FROM document_text_pages WHERE node_id = $1")
        .bind(node_id)
        .execute(&mut *transaction)
        .await?;
    let mut stored = Vec::with_capacity(pages.len());
    for page in pages {
        stored.push(
            sqlx::query_as::<_, DocumentTextPageRecord>(
                r"
                INSERT INTO document_text_pages (
                    id, node_id, page_number, content, confidence, width, height
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                RETURNING *
                ",
            )
            .bind(Uuid::new_v4())
            .bind(node_id)
            .bind(page.page_number)
            .bind(page.content)
            .bind(page.confidence)
            .bind(page.width)
            .bind(page.height)
            .fetch_one(&mut *transaction)
            .await?,
        );
    }
    transaction.commit().await?;
    Ok(stored)
}

/// Atomically upserts document-level text state and replaces every page.
///
/// # Errors
///
/// Returns a database error and rolls back the entire document/page update when any row fails.
pub async fn replace_document_text(
    pool: &PgPool,
    input: &UpsertDocumentText<'_>,
    pages: &[DocumentTextPageInput<'_>],
) -> Result<DocumentTextRecord, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let record = sqlx::query_as::<_, DocumentTextRecord>(
        r"
        INSERT INTO document_text (
            node_id, source, status, language, engine_name, engine_version,
            page_count, mean_confidence, char_count, warnings, duration_ms
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (node_id) DO UPDATE SET
            source = EXCLUDED.source, status = EXCLUDED.status,
            language = EXCLUDED.language, engine_name = EXCLUDED.engine_name,
            engine_version = EXCLUDED.engine_version, page_count = EXCLUDED.page_count,
            mean_confidence = EXCLUDED.mean_confidence, char_count = EXCLUDED.char_count,
            warnings = EXCLUDED.warnings, duration_ms = EXCLUDED.duration_ms,
            updated_at = now()
        RETURNING *
        ",
    )
    .bind(input.node_id)
    .bind(input.source)
    .bind(input.status)
    .bind(input.language)
    .bind(input.engine_name)
    .bind(input.engine_version)
    .bind(input.page_count)
    .bind(input.mean_confidence)
    .bind(input.char_count)
    .bind(input.warnings)
    .bind(input.duration_ms)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM document_text_pages WHERE node_id = $1")
        .bind(input.node_id)
        .execute(&mut *transaction)
        .await?;
    for page in pages {
        sqlx::query(
            r"
            INSERT INTO document_text_pages (
                id, node_id, page_number, content, confidence, width, height
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            ",
        )
        .bind(Uuid::new_v4())
        .bind(input.node_id)
        .bind(page.page_number)
        .bind(page.content)
        .bind(page.confidence)
        .bind(page.width)
        .bind(page.height)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(record)
}

/// Fetches the document-level text state for one node.
///
/// # Errors
///
/// Returns a database error when the record cannot be queried.
pub async fn get_document_text(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Option<DocumentTextRecord>, sqlx::Error> {
    sqlx::query_as::<_, DocumentTextRecord>("SELECT * FROM document_text WHERE node_id = $1")
        .bind(node_id)
        .fetch_optional(pool)
        .await
}

/// Lists stored pages for one node in source order.
///
/// # Errors
///
/// Returns a database error when pages cannot be queried.
pub async fn list_document_text_pages(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Vec<DocumentTextPageRecord>, sqlx::Error> {
    sqlx::query_as::<_, DocumentTextPageRecord>(
        "SELECT * FROM document_text_pages WHERE node_id = $1 ORDER BY page_number",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await
}

/// Counts document text records by durable status.
///
/// # Errors
///
/// Returns a database error when status counts cannot be queried.
pub async fn count_document_text_by_status(
    pool: &PgPool,
) -> Result<Vec<DocumentTextStatusCount>, sqlx::Error> {
    sqlx::query_as::<_, DocumentTextStatusCount>(
        r"
        SELECT status, count(*) AS count
        FROM document_text
        GROUP BY status
        ORDER BY status
        ",
    )
    .fetch_all(pool)
    .await
}

/// Records the OCR engine identity verified by the active worker.
///
/// # Errors
///
/// Returns a database error when the singleton state cannot be persisted.
pub async fn set_ocr_engine_state(
    pool: &PgPool,
    engine_name: &str,
    engine_version: &str,
    language: &str,
) -> Result<OcrEngineState, sqlx::Error> {
    sqlx::query_as::<_, OcrEngineState>(
        r"
        INSERT INTO ocr_engine_state (singleton, engine_name, engine_version, language)
        VALUES (TRUE, $1, $2, $3)
        ON CONFLICT (singleton) DO UPDATE SET
            engine_name = EXCLUDED.engine_name,
            engine_version = EXCLUDED.engine_version,
            language = EXCLUDED.language,
            updated_at = now()
        RETURNING engine_name, engine_version, language, updated_at
        ",
    )
    .bind(engine_name)
    .bind(engine_version)
    .bind(language)
    .fetch_one(pool)
    .await
}

/// Loads the currently verified OCR engine identity.
///
/// # Errors
///
/// Returns a database error when the state cannot be queried.
pub async fn get_ocr_engine_state(pool: &PgPool) -> Result<Option<OcrEngineState>, sqlx::Error> {
    sqlx::query_as(
        "SELECT engine_name, engine_version, language, updated_at FROM ocr_engine_state WHERE singleton",
    )
    .fetch_optional(pool)
    .await
}

/// Appends a bounded OCR lifecycle event and returns its cursor-bearing record.
///
/// # Errors
///
/// Returns a database error when the event cannot be written.
pub async fn append_ocr_event(
    pool: &PgPool,
    node_id: Uuid,
    state: &str,
    page_count: Option<i32>,
    mean_confidence: Option<f32>,
    warning: Option<&str>,
) -> Result<OcrEventRecord, sqlx::Error> {
    sqlx::query_as::<_, OcrEventRecord>(
        r"
        INSERT INTO ocr_events (
            node_id, node_name, state, page_count, mean_confidence, warning
        )
        SELECT id, name, $2, $3, $4, $5 FROM nodes WHERE id = $1
        RETURNING id, node_id, node_name, state, page_count, mean_confidence,
                  warning, created_at
        ",
    )
    .bind(node_id)
    .bind(state)
    .bind(page_count)
    .bind(mean_confidence)
    .bind(warning)
    .fetch_one(pool)
    .await
}

/// Lists OCR events after a monotonically increasing cursor.
///
/// # Errors
///
/// Returns a database error when event history cannot be queried.
pub async fn list_ocr_events_after(
    pool: &PgPool,
    after_id: i64,
    limit: i64,
) -> Result<Vec<OcrEventRecord>, sqlx::Error> {
    sqlx::query_as::<_, OcrEventRecord>(
        r"
        SELECT id, node_id, node_name, state, page_count, mean_confidence,
               warning, created_at
        FROM ocr_events
        WHERE id > $1
        ORDER BY id
        LIMIT $2
        ",
    )
    .bind(after_id)
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await
}

/// Counts metadata extraction jobs and the recent completion rate.
///
/// # Errors
///
/// Returns a database error when the queue or event history cannot be queried.
pub async fn get_metadata_status_counts(
    pool: &PgPool,
) -> Result<MetadataStatusCounts, sqlx::Error> {
    sqlx::query_as::<_, MetadataStatusCounts>(
        r"
        SELECT
            count(*) FILTER (WHERE state = 'pending') AS pending,
            count(*) FILTER (WHERE state = 'leased') AS running,
            count(*) FILTER (WHERE state = 'completed') AS completed,
            count(*) FILTER (WHERE state = 'failed') AS failed,
            count(*) FILTER (WHERE state = 'skipped') AS skipped,
            count(*) FILTER (WHERE state = 'cancelled') AS cancelled,
            count(*) FILTER (WHERE state IN ('pending', 'leased')) AS remaining,
            count(*) AS total,
            (
                SELECT count(*) * 12
                FROM metadata_events
                WHERE state = 'completed'
                  AND created_at >= now() - interval '5 minutes'
            ) AS completed_per_hour
        FROM jobs
        WHERE job_type = 'metadata_extraction'
        ",
    )
    .fetch_one(pool)
    .await
}

/// Lists metadata console events after a monotonically increasing cursor.
///
/// # Errors
///
/// Returns a database error when event history cannot be queried.
pub async fn list_metadata_events_after(
    pool: &PgPool,
    after_id: i64,
    limit: i64,
) -> Result<Vec<MetadataEventRecord>, sqlx::Error> {
    sqlx::query_as::<_, MetadataEventRecord>(
        r"
        SELECT id, job_id, node_id, node_name, state, attempt,
               extractor_name, duration_ms, error_message, created_at
        FROM metadata_events
        WHERE id > $1
        ORDER BY id
        LIMIT $2
        ",
    )
    .bind(after_id)
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await
}

/// Lists the newest metadata console events in reverse chronological order.
///
/// # Errors
///
/// Returns a database error when event history cannot be queried.
pub async fn list_recent_metadata_events(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<MetadataEventRecord>, sqlx::Error> {
    sqlx::query_as::<_, MetadataEventRecord>(
        r"
        SELECT id, job_id, node_id, node_name, state, attempt,
               extractor_name, duration_ms, error_message, created_at
        FROM metadata_events
        ORDER BY id DESC
        LIMIT $1
        ",
    )
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await
}

/// Searches indexed English document text and returns stable relevance-ranked pages.
///
/// The cursor is the prior page row identifier; its score is recovered inside the query so
/// pagination remains stable without exposing an offset.
///
/// # Errors
///
/// Returns a database error when the full-text query cannot be executed.
pub async fn search_document_text(
    pool: &PgPool,
    query: &str,
    include_trash: bool,
    cursor: Option<Uuid>,
    limit: u32,
) -> Result<Vec<DocumentTextSearchResult>, sqlx::Error> {
    sqlx::query_as::<_, DocumentTextSearchResult>(
        r"
        WITH search_query AS (
            SELECT websearch_to_tsquery('english', $1) AS value
        ),
        ranked AS (
            SELECT
                p.id AS cursor_id,
                n.id AS node_id,
                n.name AS node_name,
                p.page_number,
                ts_headline(
                    'english', p.content, search_query.value,
                    'StartSel=<<strife>>, StopSel=<</strife>>, MaxFragments=3, MaxWords=35, MinWords=12'
                ) AS snippet,
                ts_rank_cd(p.search_vector, search_query.value) AS score
            FROM document_text_pages p
            JOIN nodes n ON n.id = p.node_id
            CROSS JOIN search_query
            WHERE p.search_vector @@ search_query.value
              AND (
                  (NOT $2 AND n.lifecycle_state = 'active')
                  OR ($2 AND n.lifecycle_state <> 'deleted')
              )
        ),
        cursor_row AS (
            SELECT score, cursor_id FROM ranked WHERE cursor_id = $3
        )
        SELECT cursor_id, node_id, node_name, page_number, snippet, score
        FROM ranked
        WHERE $3::uuid IS NULL
           OR score < (SELECT score FROM cursor_row)
           OR (
               score = (SELECT score FROM cursor_row)
               AND cursor_id > (SELECT cursor_id FROM cursor_row)
           )
        ORDER BY score DESC, cursor_id
        LIMIT $4
        ",
    )
    .bind(query)
    .bind(include_trash)
    .bind(cursor)
    .bind(i64::from(limit.clamp(1, 100)))
    .fetch_all(pool)
    .await
}

/// Loads OCR status using indexed SQL aggregates without materializing job or text rows.
///
/// # Errors
///
/// Returns a database error when the aggregate query cannot run.
pub async fn get_ocr_status_counts(pool: &PgPool) -> Result<OcrStatusCounts, sqlx::Error> {
    sqlx::query_as::<_, OcrStatusCounts>(
        r"
        SELECT
            (SELECT count(*) FROM jobs WHERE job_type = 'ocr' AND state = 'pending') AS pending,
            (SELECT count(*) FROM jobs WHERE job_type = 'ocr' AND state = 'leased') AS running,
            (SELECT count(*) FROM document_text WHERE status = 'completed' AND source = 'ocr') AS completed,
            (SELECT count(*) FROM document_text WHERE status = 'failed') AS failed,
            (SELECT count(*) FROM document_text WHERE status = 'skipped' OR (status = 'completed' AND source = 'embedded')) AS skipped,
            (SELECT count(*) FROM document_text WHERE status = 'unsupported') AS unsupported,
            (SELECT count(*) FROM jobs WHERE job_type = 'ocr' AND state IN ('pending', 'leased')) AS remaining
        ",
    )
    .fetch_one(pool)
    .await
}

/// Lifecycle of one email parsing attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "email_extraction_status", rename_all = "snake_case")]
pub enum EmailExtractionStatus {
    Pending,
    Completed,
    Failed,
    Skipped,
    Unsupported,
}

/// RFC role an address was written under.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "email_address_role", rename_all = "snake_case")]
pub enum EmailAddressRole {
    From,
    Sender,
    ReplyTo,
    To,
    Cc,
    Bcc,
}

/// Parsed projection of one `.eml` node.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct EmailMessageRecord {
    pub node_id: Uuid,
    pub status: EmailExtractionStatus,
    pub parser_name: String,
    pub parser_version: String,
    pub message_id: Option<String>,
    pub normalized_message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub reference_ids: Vec<String>,
    pub subject: Option<String>,
    pub normalized_subject: Option<String>,
    pub sent_at: Option<DateTime<Utc>>,
    pub received_at: Option<DateTime<Utc>>,
    pub body_text: String,
    pub body_html: Option<String>,
    pub preview_text: String,
    pub content_hash: Option<String>,
    pub thread_group_id: Option<Uuid>,
    pub thread_reason: EmailThreadReason,
    pub thread_conflict: bool,
    pub duplicate_group_id: Option<Uuid>,
    pub duplicate_reason: EmailDuplicateReason,
    pub provider_thread_id: Option<String>,
    pub attachment_count: i32,
    pub warnings: Vec<String>,
    pub duration_ms: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct EmailAddressRecord {
    pub id: i64,
    pub node_id: Uuid,
    pub role: EmailAddressRole,
    pub position: i32,
    pub display_name: Option<String>,
    pub address: String,
}

#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct EmailHeaderRecord {
    pub id: i64,
    pub node_id: Uuid,
    pub position: i32,
    pub name: String,
    pub normalized_name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct EmailAttachmentRecord {
    pub id: i64,
    pub node_id: Uuid,
    pub part_path: String,
    pub position: i32,
    pub filename: Option<String>,
    pub media_type: String,
    pub disposition: Option<String>,
    pub content_id: Option<String>,
    pub transfer_encoding: Option<String>,
    pub decoded_size: Option<i64>,
    pub checksum_sha256: Option<String>,
    pub is_inline: bool,
    pub is_message: bool,
    pub extraction_status: EmailExtractionStatus,
    pub warnings: Vec<String>,
}

/// Message-level values persisted by one parsing attempt.
pub struct UpsertEmailMessage<'a> {
    pub node_id: Uuid,
    pub status: EmailExtractionStatus,
    pub parser_name: &'a str,
    pub parser_version: &'a str,
    pub message_id: Option<&'a str>,
    pub normalized_message_id: Option<&'a str>,
    pub in_reply_to: Option<&'a str>,
    pub reference_ids: &'a [String],
    pub subject: Option<&'a str>,
    pub normalized_subject: Option<&'a str>,
    pub sent_at: Option<DateTime<Utc>>,
    pub received_at: Option<DateTime<Utc>>,
    pub body_text: &'a str,
    pub body_html: Option<&'a str>,
    pub preview_text: &'a str,
    pub content_hash: Option<&'a str>,
    pub provider_thread_id: Option<&'a str>,
    pub warnings: &'a [String],
    pub duration_ms: Option<i64>,
}

pub struct EmailAddressInput<'a> {
    pub role: EmailAddressRole,
    pub display_name: Option<&'a str>,
    pub address: &'a str,
}

pub struct EmailHeaderInput<'a> {
    pub name: &'a str,
    pub value: &'a str,
}

pub struct EmailAttachmentInput<'a> {
    pub part_path: &'a str,
    pub filename: Option<&'a str>,
    pub media_type: &'a str,
    pub disposition: Option<&'a str>,
    pub content_id: Option<&'a str>,
    pub transfer_encoding: Option<&'a str>,
    pub decoded_size: Option<i64>,
    pub checksum_sha256: Option<&'a str>,
    pub is_inline: bool,
    pub is_message: bool,
    pub warnings: &'a [String],
}

/// Everything one parse produces, replaced together or not at all.
pub struct EmailProjection<'a> {
    pub message: UpsertEmailMessage<'a>,
    pub addresses: &'a [EmailAddressInput<'a>],
    pub headers: &'a [EmailHeaderInput<'a>],
    pub labels: &'a [String],
    pub attachments: &'a [EmailAttachmentInput<'a>],
}

/// Atomically replaces a message and all of its dependent rows.
///
/// Reparsing must never leave a message carrying addresses from one parser
/// version and attachments from another, so the delete-and-insert of every
/// dependent table shares the message upsert's transaction.
///
/// # Errors
///
/// Returns a database error when the replacement transaction cannot commit.
pub async fn replace_email_projection(
    pool: &PgPool,
    projection: &EmailProjection<'_>,
) -> Result<EmailMessageRecord, sqlx::Error> {
    let input = &projection.message;
    // Grouping is derived here rather than supplied by the caller, so a thread
    // or duplicate assignment cannot drift out of step with the headers it is
    // computed from.
    let grouping = group_email(&GroupingEvidence {
        provider_thread_id: input.provider_thread_id,
        normalized_message_id: input.normalized_message_id,
        in_reply_to: input.in_reply_to,
        reference_ids: input.reference_ids,
        normalized_subject: input.normalized_subject,
        content_hash: input.content_hash,
    });
    let mut transaction = pool.begin().await?;
    let message = sqlx::query_as::<_, EmailMessageRecord>(
        r"
        INSERT INTO email_messages (
            node_id, status, parser_name, parser_version, message_id,
            normalized_message_id, in_reply_to, reference_ids, subject,
            normalized_subject, sent_at, received_at, body_text, body_html,
            preview_text, content_hash, provider_thread_id, attachment_count,
            warnings, duration_ms, thread_group_id, thread_reason,
            thread_conflict, duplicate_group_id, duplicate_reason
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25)
        ON CONFLICT (node_id) DO UPDATE SET
            status = EXCLUDED.status,
            parser_name = EXCLUDED.parser_name,
            parser_version = EXCLUDED.parser_version,
            message_id = EXCLUDED.message_id,
            normalized_message_id = EXCLUDED.normalized_message_id,
            in_reply_to = EXCLUDED.in_reply_to,
            reference_ids = EXCLUDED.reference_ids,
            subject = EXCLUDED.subject,
            normalized_subject = EXCLUDED.normalized_subject,
            sent_at = EXCLUDED.sent_at,
            received_at = EXCLUDED.received_at,
            body_text = EXCLUDED.body_text,
            body_html = EXCLUDED.body_html,
            preview_text = EXCLUDED.preview_text,
            content_hash = EXCLUDED.content_hash,
            provider_thread_id = EXCLUDED.provider_thread_id,
            attachment_count = EXCLUDED.attachment_count,
            warnings = EXCLUDED.warnings,
            duration_ms = EXCLUDED.duration_ms,
            thread_group_id = EXCLUDED.thread_group_id,
            thread_reason = EXCLUDED.thread_reason,
            thread_conflict = EXCLUDED.thread_conflict,
            duplicate_group_id = EXCLUDED.duplicate_group_id,
            duplicate_reason = EXCLUDED.duplicate_reason,
            updated_at = now()
        RETURNING *
        ",
    )
    .bind(input.node_id)
    .bind(input.status)
    .bind(input.parser_name)
    .bind(input.parser_version)
    .bind(input.message_id)
    .bind(input.normalized_message_id)
    .bind(input.in_reply_to)
    .bind(input.reference_ids)
    .bind(input.subject)
    .bind(input.normalized_subject)
    .bind(input.sent_at)
    .bind(input.received_at)
    .bind(input.body_text)
    .bind(input.body_html)
    .bind(input.preview_text)
    .bind(input.content_hash)
    .bind(input.provider_thread_id)
    .bind(i32::try_from(projection.attachments.len()).unwrap_or(i32::MAX))
    .bind(input.warnings)
    .bind(input.duration_ms)
    .bind(grouping.thread_group_id)
    .bind(grouping.thread_reason)
    .bind(grouping.thread_conflict)
    .bind(grouping.duplicate_group_id)
    .bind(grouping.duplicate_reason)
    .fetch_one(&mut *transaction)
    .await?;

    replace_email_dependents(&mut transaction, input.node_id, projection).await?;
    transaction.commit().await?;
    Ok(message)
}

/// Deletes and reinserts every row that depends on a message, in one caller
/// transaction so a reparse cannot mix parser versions across tables.
async fn replace_email_dependents(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
    projection: &EmailProjection<'_>,
) -> Result<(), sqlx::Error> {
    for table in [
        "DELETE FROM email_addresses WHERE node_id = $1",
        "DELETE FROM email_headers WHERE node_id = $1",
        "DELETE FROM email_labels WHERE node_id = $1",
        "DELETE FROM email_attachments WHERE node_id = $1",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(table))
            .bind(node_id)
            .execute(&mut **transaction)
            .await?;
    }

    for (position, address) in projection.addresses.iter().enumerate() {
        sqlx::query(
            r"
            INSERT INTO email_addresses (node_id, role, position, display_name, address)
            VALUES ($1, $2, $3, $4, $5)
            ",
        )
        .bind(node_id)
        .bind(address.role)
        .bind(i32::try_from(position).unwrap_or(i32::MAX))
        .bind(address.display_name)
        .bind(address.address)
        .execute(&mut **transaction)
        .await?;
    }

    for (position, header) in projection.headers.iter().enumerate() {
        sqlx::query(
            r"
            INSERT INTO email_headers (node_id, position, name, normalized_name, value)
            VALUES ($1, $2, $3, lower($3), $4)
            ",
        )
        .bind(node_id)
        .bind(i32::try_from(position).unwrap_or(i32::MAX))
        .bind(header.name)
        .bind(header.value)
        .execute(&mut **transaction)
        .await?;
    }

    for label in projection.labels {
        sqlx::query(
            "INSERT INTO email_labels (node_id, label) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(node_id)
        .bind(label)
        .execute(&mut **transaction)
        .await?;
    }

    for (position, attachment) in projection.attachments.iter().enumerate() {
        sqlx::query(
            r"
            INSERT INTO email_attachments (
                node_id, part_path, position, filename, media_type, disposition,
                content_id, transfer_encoding, decoded_size, checksum_sha256,
                is_inline, is_message, warnings
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ",
        )
        .bind(node_id)
        .bind(attachment.part_path)
        .bind(i32::try_from(position).unwrap_or(i32::MAX))
        .bind(attachment.filename)
        .bind(attachment.media_type)
        .bind(attachment.disposition)
        .bind(attachment.content_id)
        .bind(attachment.transfer_encoding)
        .bind(attachment.decoded_size)
        .bind(attachment.checksum_sha256)
        .bind(attachment.is_inline)
        .bind(attachment.is_message)
        .bind(attachment.warnings)
        .execute(&mut **transaction)
        .await?;
    }

    Ok(())
}

/// One email extraction console event.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct EmailEventRecord {
    pub id: i64,
    pub node_id: Option<Uuid>,
    pub node_name: String,
    pub state: String,
    pub subject: Option<String>,
    pub attachment_count: Option<i32>,
    pub duration_ms: Option<i64>,
    pub origin: JobOrigin,
    pub campaign_id: Option<Uuid>,
    pub warning: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Longest subject kept on an event row.
///
/// The console needs enough to recognise a message, not the whole header. A
/// bounded copy also keeps one pathological subject from dominating the table.
const EMAIL_EVENT_SUBJECT_LIMIT: usize = 120;

/// Records one email extraction outcome for the operator console.
///
/// Carries identifiers and measurements only. Body text, addresses, and raw
/// headers are never written here: these rows are retained indefinitely and
/// displayed live, so anything sensitive in them would leak into both.
///
/// # Errors
///
/// Returns a database error when the event cannot be inserted.
#[allow(clippy::too_many_arguments)]
pub async fn record_email_event(
    pool: &PgPool,
    node_id: Option<Uuid>,
    node_name: &str,
    state: &str,
    subject: Option<&str>,
    attachment_count: Option<i32>,
    duration_ms: Option<i64>,
    origin: JobOrigin,
    campaign_id: Option<Uuid>,
    warning: Option<&str>,
) -> Result<EmailEventRecord, sqlx::Error> {
    let subject = subject.map(|value| {
        let mut end = value.len().min(EMAIL_EVENT_SUBJECT_LIMIT);
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        &value[..end]
    });
    sqlx::query_as::<_, EmailEventRecord>(
        r"
        INSERT INTO email_events (
            node_id, node_name, state, subject, attachment_count, duration_ms,
            origin, campaign_id, warning
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING *
        ",
    )
    .bind(node_id)
    .bind(node_name)
    .bind(state)
    .bind(subject)
    .bind(attachment_count)
    .bind(duration_ms)
    .bind(origin)
    .bind(campaign_id)
    .bind(warning)
    .fetch_one(pool)
    .await
}

/// Lists email events after a monotonically increasing cursor.
///
/// # Errors
///
/// Returns a database error when event history cannot be queried.
pub async fn list_email_events_after(
    pool: &PgPool,
    after_id: i64,
    limit: i64,
) -> Result<Vec<EmailEventRecord>, sqlx::Error> {
    sqlx::query_as::<_, EmailEventRecord>(
        "SELECT * FROM email_events WHERE id > $1 ORDER BY id LIMIT $2",
    )
    .bind(after_id)
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await
}

/// One version value and how many rows carry it.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct VersionCount {
    pub version: String,
    pub count: i64,
}

/// Version distributions across the independent reprocessing axes.
///
/// These are separate fields rather than one "email version" because a change
/// to each requires different work: a parser change means reparsing messages, a
/// sanitizer change means only re-rendering, an attachment-extractor change
/// means re-extracting text without touching the message, and a search-index
/// change means rebuilding vectors from data already stored. Collapsing them
/// would force the most expensive reprocessing for the cheapest change.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmailVersionReport {
    pub parser: Vec<VersionCount>,
    pub attachment_materializer: Vec<VersionCount>,
    pub attachment_extractor: Vec<VersionCount>,
    /// Messages whose parser version differs from the running one.
    pub messages_needing_reparse: i64,
    /// Attachments whose extractor version differs from the running one.
    pub attachments_needing_reextraction: i64,
    /// Completed messages with no search vector.
    pub messages_needing_reindex: i64,
}

/// Reports version distributions and how much reprocessing each implies.
///
/// # Errors
///
/// Returns a database error when an aggregate query cannot run.
pub async fn email_version_report(
    pool: &PgPool,
    parser_version: &str,
    attachment_extractor_version: &str,
) -> Result<EmailVersionReport, sqlx::Error> {
    let parser = sqlx::query_as::<_, VersionCount>(
        "SELECT parser_version AS version, count(*) AS count
         FROM email_messages GROUP BY parser_version ORDER BY count DESC",
    )
    .fetch_all(pool)
    .await?;
    let attachment_materializer = sqlx::query_as::<_, VersionCount>(
        "SELECT materializer_version AS version, count(*) AS count
         FROM email_attachment_artifacts GROUP BY materializer_version ORDER BY count DESC",
    )
    .fetch_all(pool)
    .await?;
    let attachment_extractor = sqlx::query_as::<_, VersionCount>(
        "SELECT coalesce(text_extractor_version, '(none)') AS version, count(*) AS count
         FROM email_attachment_artifacts GROUP BY text_extractor_version ORDER BY count DESC",
    )
    .fetch_all(pool)
    .await?;

    let messages_needing_reparse = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM email_messages WHERE parser_version IS DISTINCT FROM $1",
    )
    .bind(parser_version)
    .fetch_one(pool)
    .await?;
    let attachments_needing_reextraction = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM email_attachment_artifacts
         WHERE state = 'ready' AND text_status = 'completed'
           AND text_extractor_version IS DISTINCT FROM $1",
    )
    .bind(attachment_extractor_version)
    .fetch_one(pool)
    .await?;
    let messages_needing_reindex = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM email_messages
         WHERE status = 'completed' AND search_vector IS NULL",
    )
    .fetch_one(pool)
    .await?;

    Ok(EmailVersionReport {
        parser,
        attachment_materializer,
        attachment_extractor,
        messages_needing_reparse,
        attachments_needing_reextraction,
        messages_needing_reindex,
    })
}

/// What a repair scan found, without changing anything.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmailRepairReport {
    /// Finalized `.eml`-shaped nodes with no projection at all.
    pub missing_projections: i64,
    /// Artifact rows whose message no longer exists.
    pub orphan_artifacts: i64,
    /// Manifest entries with no artifact, and artifacts with no manifest entry.
    pub manifest_without_artifact: i64,
    pub artifact_without_manifest: i64,
    /// Jobs leased past their expiry, which a live worker would have renewed.
    pub stale_leases: i64,
    /// Campaigns whose recorded counts disagree with the jobs table.
    pub campaigns_with_count_drift: i64,
    pub messages_needing_reindex: i64,
}

/// Scans for inconsistencies without mutating anything.
///
/// Read-only by construction: there is no write path in this function, so
/// "dry run" is not a flag that could be passed wrongly. Acting on what it finds
/// is a separate, explicitly scoped call.
///
/// # Errors
///
/// Returns a database error when a scan query cannot run.
pub async fn email_repair_scan(pool: &PgPool) -> Result<EmailRepairReport, sqlx::Error> {
    let missing_projections = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM nodes n
         JOIN file_objects f ON f.node_id = n.id AND f.upload_state = 'finalized'
         LEFT JOIN email_messages e ON e.node_id = n.id
         WHERE n.kind = 'file' AND n.lifecycle_state = 'active' AND e.node_id IS NULL
           AND n.name ILIKE '%.eml'",
    )
    .fetch_one(pool)
    .await?;
    // The foreign key makes true orphans impossible; the check stays so a future
    // schema change that drops the cascade is caught rather than assumed safe.
    let orphan_artifacts = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM email_attachment_artifacts a
         LEFT JOIN email_messages e ON e.node_id = a.node_id
         WHERE e.node_id IS NULL",
    )
    .fetch_one(pool)
    .await?;
    let manifest_without_artifact = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM email_attachments m
         LEFT JOIN email_attachment_artifacts a
                ON a.node_id = m.node_id AND a.part_path = m.part_path
         WHERE a.id IS NULL",
    )
    .fetch_one(pool)
    .await?;
    let artifact_without_manifest = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM email_attachment_artifacts a
         LEFT JOIN email_attachments m
                ON m.node_id = a.node_id AND m.part_path = a.part_path
         WHERE m.id IS NULL",
    )
    .fetch_one(pool)
    .await?;
    let stale_leases = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM jobs
         WHERE state = 'leased' AND lease_expires_at IS NOT NULL
           AND lease_expires_at < now()",
    )
    .fetch_one(pool)
    .await?;
    let campaigns_with_count_drift = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM backfill_campaigns c
         WHERE c.enqueued_count <
               (SELECT count(*) FROM jobs j WHERE j.campaign_id = c.id)",
    )
    .fetch_one(pool)
    .await?;
    let messages_needing_reindex = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM email_messages
         WHERE status = 'completed' AND search_vector IS NULL",
    )
    .fetch_one(pool)
    .await?;

    Ok(EmailRepairReport {
        missing_projections,
        orphan_artifacts,
        manifest_without_artifact,
        artifact_without_manifest,
        stale_leases,
        campaigns_with_count_drift,
        messages_needing_reindex,
    })
}

/// Reconciles a campaign's recorded counts with the jobs table.
///
/// Deliberately cannot change campaign *state*. Repairing a count is a
/// bookkeeping fix; resuming a paused campaign is an operational decision, and a
/// repair command that could do the second as a side effect of the first would
/// be able to start a ten-year backfill nobody authorized.
///
/// # Errors
///
/// Returns a database error when the campaign cannot be reconciled.
pub async fn repair_campaign_counts(
    pool: &PgPool,
    campaign_id: Uuid,
) -> Result<BackfillCampaignRecord, sqlx::Error> {
    sqlx::query_as::<_, BackfillCampaignRecord>(
        r"
        UPDATE backfill_campaigns c
        SET enqueued_count = GREATEST(
                c.enqueued_count,
                (SELECT count(*) FROM jobs j WHERE j.campaign_id = c.id)
            ),
            updated_at = now()
        WHERE c.id = $1
        RETURNING *
        ",
    )
    .bind(campaign_id)
    .fetch_one(pool)
    .await
}

/// Deletes finished jobs past their retention window, in one bounded batch.
///
/// Retention is per outcome rather than uniform, because the two kinds of row
/// have different value. A `completed` job is a receipt nobody reads: it proves
/// work happened and is superseded by the artifact it produced. A `failed` or
/// `cancelled` job is evidence — it carries `last_error` and the attempt count,
/// which is what an operator triaging a bad import or a parser regression
/// actually reads. So successes are dropped early and failures are kept long.
///
/// Three things are never deleted:
///
/// - `pending` and `leased` jobs, at any age. A leased job whose worker died is
///   recovered by lease expiry, not by deletion; removing it would strand the
///   work permanently.
/// - Failed jobs whose target still has an unresolved `failed` import entry.
///   That entry is what the Actionable Errors tab lists, and its retry needs the
///   job's error context to explain what went wrong.
/// - Anything beyond `batch`, so a first run against a long-neglected table
///   takes many small bites instead of locking it for one large one.
///
/// Returns how many rows were removed.
///
/// # Errors
///
/// Returns a database error when the purge cannot run.
pub async fn purge_expired_jobs(
    pool: &PgPool,
    completed_retention_days: i32,
    failed_retention_days: i32,
    batch: u32,
) -> Result<u64, sqlx::Error> {
    let batch = i64::from(batch.clamp(1, 10_000));
    let result = sqlx::query(
        r"
        WITH expired AS (
            SELECT j.id
            FROM jobs j
            WHERE j.state IN ('completed', 'skipped', 'failed', 'cancelled')
              AND coalesce(j.completed_at, j.updated_at) < now() - (
                  CASE
                      WHEN j.state IN ('completed', 'skipped') THEN $1
                      ELSE $2
                  END * interval '1 day'
              )
              -- The Actionable Errors tab lists unresolved failed imports and
              -- offers a retry; deleting the job behind one would leave a row
              -- the user can see but nothing can explain.
              AND NOT (
                  j.state = 'failed'
                  AND EXISTS (
                      SELECT 1 FROM import_entries e
                      WHERE e.resulting_node_id = j.target_node_id
                        AND e.state = 'failed'
                  )
              )
            ORDER BY coalesce(j.completed_at, j.updated_at)
            LIMIT $3
            FOR UPDATE SKIP LOCKED
        )
        DELETE FROM jobs WHERE id IN (SELECT id FROM expired)
        ",
    )
    .bind(completed_retention_days)
    .bind(failed_retention_days)
    .bind(batch)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Reconciles how many concurrent slots a resource class offers.
///
/// The permit is a set of rows rather than an advisory lock precisely so it can
/// be resized without a deployment and so it survives a worker that dies mid-job
/// — an expired lease frees its slot, where a held lock would not.
///
/// Shrinking never revokes a slot that is currently leased: the extra rows are
/// removed only when free, so a running job is never orphaned by a config
/// change. A shrink that cannot complete now completes on the next call.
///
/// # Errors
///
/// Returns a database error when slots cannot be reconciled.
pub async fn set_resource_slots(
    pool: &PgPool,
    resource_class: JobResourceClass,
    slots: i32,
) -> Result<i32, sqlx::Error> {
    let slots = slots.max(1);
    let mut transaction = pool.begin().await?;
    for slot_number in 1..=slots {
        sqlx::query(
            "INSERT INTO worker_resource_leases (resource_class, slot_number)
             VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(resource_class)
        .bind(slot_number)
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query(
        "DELETE FROM worker_resource_leases
         WHERE resource_class = $1 AND slot_number > $2
           AND (lease_expires_at IS NULL OR lease_expires_at < now())",
    )
    .bind(resource_class)
    .bind(slots)
    .execute(&mut *transaction)
    .await?;
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM worker_resource_leases WHERE resource_class = $1",
    )
    .bind(resource_class)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(i32::try_from(count).unwrap_or(slots))
}

/// Aggregate email extraction workload and durable outcome counts.
///
/// Queued work is split by origin because a paused historical campaign and a
/// stalled inbox look identical in a single total: an operator seeing "12,000
/// pending" needs to know whether new mail is moving.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct EmailStatusCounts {
    pub foreground_pending: i64,
    pub foreground_running: i64,
    pub backfill_pending: i64,
    pub backfill_running: i64,
    pub completed: i64,
    pub failed: i64,
    pub skipped: i64,
    pub unsupported: i64,
    pub remaining: i64,
    pub indexed: i64,
    /// Messages that have an extraction row of any kind, so "candidates minus
    /// completed" is answerable without scanning nodes.
    pub candidates: i64,
    /// Attachment artifacts still awaiting text extraction.
    pub attachments_pending: i64,
    pub attachments_completed: i64,
    pub attachments_failed: i64,
    /// Messages parsed in the last hour, the basis for throughput and ETA.
    pub completed_last_hour: i64,
}

/// Loads email status using indexed SQL aggregates without materializing rows.
///
/// # Errors
///
/// Returns a database error when the aggregate query cannot run.
pub async fn email_status_counts(pool: &PgPool) -> Result<EmailStatusCounts, sqlx::Error> {
    sqlx::query_as::<_, EmailStatusCounts>(
        r"
        SELECT
            (SELECT count(*) FROM jobs
              WHERE job_type = 'email_extraction' AND state = 'pending'
                AND origin <> 'backfill') AS foreground_pending,
            (SELECT count(*) FROM jobs
              WHERE job_type = 'email_extraction' AND state = 'leased'
                AND origin <> 'backfill') AS foreground_running,
            (SELECT count(*) FROM jobs
              WHERE job_type = 'email_extraction' AND state = 'pending'
                AND origin = 'backfill') AS backfill_pending,
            (SELECT count(*) FROM jobs
              WHERE job_type = 'email_extraction' AND state = 'leased'
                AND origin = 'backfill') AS backfill_running,
            (SELECT count(*) FROM email_messages WHERE status = 'completed') AS completed,
            (SELECT count(*) FROM email_messages WHERE status = 'failed') AS failed,
            (SELECT count(*) FROM email_messages WHERE status = 'skipped') AS skipped,
            (SELECT count(*) FROM email_messages WHERE status = 'unsupported') AS unsupported,
            (SELECT count(*) FROM jobs
              WHERE job_type = 'email_extraction' AND state IN ('pending', 'leased')) AS remaining,
            (SELECT count(*) FROM email_messages WHERE search_vector IS NOT NULL) AS indexed,
            (SELECT count(*) FROM email_messages) AS candidates,
            (SELECT count(*) FROM email_attachment_artifacts
              WHERE state = 'ready' AND text_status = 'pending') AS attachments_pending,
            (SELECT count(*) FROM email_attachment_artifacts
              WHERE text_status = 'completed') AS attachments_completed,
            (SELECT count(*) FROM email_attachment_artifacts
              WHERE text_status = 'failed') AS attachments_failed,
            -- Throughput is measured from durable outcomes rather than from a
            -- counter the worker keeps in memory, so a restart cannot reset it.
            (SELECT count(*) FROM email_messages
              WHERE status = 'completed' AND updated_at > now() - interval '1 hour')
                AS completed_last_hour
        ",
    )
    .fetch_one(pool)
    .await
}

/// Fetches one parsed message.
///
/// # Errors
///
/// Returns a database error when the message cannot be queried.
pub async fn get_email_message(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Option<EmailMessageRecord>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM email_messages WHERE node_id = $1")
        .bind(node_id)
        .fetch_optional(pool)
        .await
}

/// Lists a message's addresses in stable role and position order.
///
/// # Errors
///
/// Returns a database error when addresses cannot be queried.
pub async fn list_email_addresses(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Vec<EmailAddressRecord>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM email_addresses WHERE node_id = $1 ORDER BY position, id")
        .bind(node_id)
        .fetch_all(pool)
        .await
}

/// Lists a message's headers in original order, repeats preserved.
///
/// # Errors
///
/// Returns a database error when headers cannot be queried.
pub async fn list_email_headers(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Vec<EmailHeaderRecord>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM email_headers WHERE node_id = $1 ORDER BY position")
        .bind(node_id)
        .fetch_all(pool)
        .await
}

/// Lists a message's Gmail labels alphabetically.
///
/// # Errors
///
/// Returns a database error when labels cannot be queried.
pub async fn list_email_labels(pool: &PgPool, node_id: Uuid) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT label FROM email_labels WHERE node_id = $1 ORDER BY label")
        .bind(node_id)
        .fetch_all(pool)
        .await
}

/// Lists a message's attachment manifest in part order.
///
/// # Errors
///
/// Returns a database error when attachments cannot be queried.
pub async fn list_email_attachments(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Vec<EmailAttachmentRecord>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM email_attachments WHERE node_id = $1 ORDER BY position, id")
        .bind(node_id)
        .fetch_all(pool)
        .await
}

/// Lifecycle of one materialized attachment artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "email_artifact_state", rename_all = "snake_case")]
pub enum EmailArtifactState {
    Pending,
    Ready,
    Failed,
    Skipped,
}

/// Namespace for deterministic attachment artifact ids.
///
/// Fixed forever: changing it would orphan every artifact already written, and
/// the identity it produces is what makes a rerun replace an artifact in place
/// rather than accumulate a second copy.
const EMAIL_ATTACHMENT_NAMESPACE: Uuid = Uuid::from_bytes([
    0x9d, 0x1a, 0x4c, 0x77, 0x2e, 0x63, 0x54, 0x8f, 0xa1, 0x0b, 0x6c, 0x39, 0x5e, 0x87, 0x12, 0xd4,
]);

/// Derives an attachment artifact's stable id from its message and MIME path.
///
/// Deliberately excludes the filename. A sender controls the filename and could
/// otherwise steer two different attachments — or two different messages — onto
/// one storage object, or influence where bytes are written at all.
#[must_use]
pub fn email_attachment_artifact_id(node_id: Uuid, part_path: &str) -> Uuid {
    let name = format!("{node_id}:{part_path}");
    Uuid::new_v5(&EMAIL_ATTACHMENT_NAMESPACE, name.as_bytes())
}

/// A materialized attachment artifact.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct EmailAttachmentArtifact {
    pub id: Uuid,
    pub node_id: Uuid,
    pub part_path: String,
    pub state: EmailArtifactState,
    pub storage_key: Option<String>,
    pub media_type: String,
    pub byte_size: i64,
    pub checksum_sha256: Option<String>,
    pub depth: i32,
    pub is_message: bool,
    pub materializer_version: String,
    pub warnings: Vec<String>,
    /// Text-extraction outcome for this attachment, tracked separately from the
    /// materialization state: bytes can be stored long before, or without ever,
    /// yielding searchable text.
    pub text_status: EmailExtractionStatus,
    pub text_extractor_name: Option<String>,
    pub text_extractor_version: Option<String>,
    pub text_bytes: i64,
    pub text_warnings: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Values written when an artifact reaches a terminal state.
#[derive(Clone, Copy, Debug)]
pub struct UpsertEmailAttachmentArtifact<'a> {
    pub node_id: Uuid,
    pub part_path: &'a str,
    pub state: EmailArtifactState,
    pub storage_key: Option<&'a str>,
    pub media_type: &'a str,
    pub byte_size: i64,
    pub checksum_sha256: Option<&'a str>,
    pub depth: i32,
    pub is_message: bool,
    pub materializer_version: &'a str,
    pub warnings: &'a [String],
}

/// Records one attachment artifact, replacing any previous row for that part.
///
/// # Errors
///
/// Returns a database error when the artifact cannot be written.
pub async fn upsert_email_attachment_artifact(
    pool: &PgPool,
    artifact: &UpsertEmailAttachmentArtifact<'_>,
) -> Result<EmailAttachmentArtifact, sqlx::Error> {
    sqlx::query_as::<_, EmailAttachmentArtifact>(
        r"
        INSERT INTO email_attachment_artifacts (
            id, node_id, part_path, state, storage_key, media_type, byte_size,
            checksum_sha256, depth, is_message, materializer_version, warnings
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        ON CONFLICT (node_id, part_path) DO UPDATE SET
            state = EXCLUDED.state,
            storage_key = EXCLUDED.storage_key,
            media_type = EXCLUDED.media_type,
            byte_size = EXCLUDED.byte_size,
            checksum_sha256 = EXCLUDED.checksum_sha256,
            depth = EXCLUDED.depth,
            is_message = EXCLUDED.is_message,
            materializer_version = EXCLUDED.materializer_version,
            warnings = EXCLUDED.warnings,
            updated_at = now()
        RETURNING *
        ",
    )
    .bind(email_attachment_artifact_id(
        artifact.node_id,
        artifact.part_path,
    ))
    .bind(artifact.node_id)
    .bind(artifact.part_path)
    .bind(artifact.state)
    .bind(artifact.storage_key)
    .bind(artifact.media_type)
    .bind(artifact.byte_size)
    .bind(artifact.checksum_sha256)
    .bind(artifact.depth)
    .bind(artifact.is_message)
    .bind(artifact.materializer_version)
    .bind(artifact.warnings)
    .fetch_one(pool)
    .await
}

/// Fetches one artifact by message and MIME part path.
///
/// # Errors
///
/// Returns a database error when the artifact cannot be queried.
pub async fn get_email_attachment_artifact(
    pool: &PgPool,
    node_id: Uuid,
    part_path: &str,
) -> Result<Option<EmailAttachmentArtifact>, sqlx::Error> {
    sqlx::query_as::<_, EmailAttachmentArtifact>(
        "SELECT * FROM email_attachment_artifacts WHERE node_id = $1 AND part_path = $2",
    )
    .bind(node_id)
    .bind(part_path)
    .fetch_optional(pool)
    .await
}

/// Lists a message's artifacts in part order.
///
/// # Errors
///
/// Returns a database error when artifacts cannot be queried.
pub async fn list_email_attachment_artifacts(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Vec<EmailAttachmentArtifact>, sqlx::Error> {
    sqlx::query_as::<_, EmailAttachmentArtifact>(
        "SELECT * FROM email_attachment_artifacts WHERE node_id = $1 ORDER BY part_path",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await
}

/// Lists artifact storage keys for a message so they can be reclaimed.
///
/// # Errors
///
/// Returns a database error when keys cannot be queried.
pub async fn email_attachment_storage_keys(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT storage_key FROM email_attachment_artifacts
         WHERE node_id = $1 AND storage_key IS NOT NULL",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await
}

/// Where an attachment's text came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "email_attachment_text_source", rename_all = "snake_case")]
pub enum EmailAttachmentTextSource {
    Embedded,
    Ocr,
}

/// One page of text extracted from one attachment.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct EmailAttachmentTextPage {
    pub node_id: Uuid,
    pub part_path: String,
    pub page_number: i32,
    pub content: String,
    pub source: EmailAttachmentTextSource,
    pub confidence: Option<f32>,
}

/// One page of text to store.
#[derive(Clone, Copy, Debug)]
pub struct EmailAttachmentTextInput<'a> {
    pub page_number: i32,
    pub content: &'a str,
    pub source: EmailAttachmentTextSource,
    pub confidence: Option<f32>,
}

/// The extraction outcome recorded against an attachment.
#[derive(Clone, Copy, Debug)]
pub struct EmailAttachmentTextOutcome<'a> {
    pub status: EmailExtractionStatus,
    pub extractor_name: Option<&'a str>,
    pub extractor_version: Option<&'a str>,
    pub warnings: &'a [String],
}

/// Replaces one attachment's extracted text and records the outcome.
///
/// Text and outcome move together in one transaction, because a search vector
/// rebuilt from half-written pages would index a document that never existed.
///
/// # Errors
///
/// Returns a database error when the text cannot be replaced.
pub async fn replace_email_attachment_text(
    pool: &PgPool,
    node_id: Uuid,
    part_path: &str,
    pages: &[EmailAttachmentTextInput<'_>],
    outcome: &EmailAttachmentTextOutcome<'_>,
) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("DELETE FROM email_attachment_text WHERE node_id = $1 AND part_path = $2")
        .bind(node_id)
        .bind(part_path)
        .execute(&mut *transaction)
        .await?;

    let mut text_bytes = 0i64;
    for page in pages {
        text_bytes += i64::try_from(page.content.len()).unwrap_or(i64::MAX);
        sqlx::query(
            r"
            INSERT INTO email_attachment_text
                (node_id, part_path, page_number, content, source, confidence)
            VALUES ($1, $2, $3, $4, $5, $6)
            ",
        )
        .bind(node_id)
        .bind(part_path)
        .bind(page.page_number)
        .bind(page.content)
        .bind(page.source)
        .bind(page.confidence)
        .execute(&mut *transaction)
        .await?;
    }

    sqlx::query(
        r"
        UPDATE email_attachment_artifacts
           SET text_status = $3,
               text_extractor_name = $4,
               text_extractor_version = $5,
               text_bytes = $6,
               text_warnings = $7,
               updated_at = now()
         WHERE node_id = $1 AND part_path = $2
        ",
    )
    .bind(node_id)
    .bind(part_path)
    .bind(outcome.status)
    .bind(outcome.extractor_name)
    .bind(outcome.extractor_version)
    .bind(text_bytes)
    .bind(outcome.warnings)
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await
}

/// Lists a message's extracted attachment text in document order.
///
/// # Errors
///
/// Returns a database error when the text cannot be queried.
pub async fn list_email_attachment_text(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Vec<EmailAttachmentTextPage>, sqlx::Error> {
    sqlx::query_as::<_, EmailAttachmentTextPage>(
        "SELECT node_id, part_path, page_number, content, source, confidence
         FROM email_attachment_text WHERE node_id = $1
         ORDER BY part_path, page_number",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await
}

/// What a bounded attachment reprocessing pass should target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttachmentReprocessScope<'a> {
    /// One attachment of one message.
    Part { node_id: Uuid, part_path: &'a str },
    /// Every message holding an attachment whose extraction failed.
    Failed,
    /// Attachments that are stored but have produced no text yet.
    Missing,
    /// Attachments extracted by a version other than the current one.
    ExtractorVersion(&'a str),
}

/// Enqueues bounded attachment extraction work for the selected scope.
///
/// Returns how many messages were enqueued. Work is queued per message rather
/// than per attachment because the job queue targets nodes, and an attachment
/// is a MIME part rather than a node of its own.
///
/// # Errors
///
/// Returns a database error when candidates cannot be selected or enqueued.
pub async fn enqueue_attachment_reprocessing(
    pool: &PgPool,
    scope: AttachmentReprocessScope<'_>,
    limit: i64,
) -> Result<u64, sqlx::Error> {
    let nodes: Vec<Uuid> = match scope {
        AttachmentReprocessScope::Part { node_id, part_path } => {
            // Resetting the row is what makes the rerun visible: the handler
            // selects pending attachments, so a completed one would be skipped.
            sqlx::query(
                "UPDATE email_attachment_artifacts SET text_status = 'pending', updated_at = now()
                 WHERE node_id = $1 AND part_path = $2",
            )
            .bind(node_id)
            .bind(part_path)
            .execute(pool)
            .await?;
            vec![node_id]
        }
        AttachmentReprocessScope::Failed => {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT DISTINCT node_id FROM email_attachment_artifacts
                 WHERE text_status = 'failed' ORDER BY node_id LIMIT $1",
            )
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        AttachmentReprocessScope::Missing => {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT DISTINCT node_id FROM email_attachment_artifacts
                 WHERE state = 'ready' AND text_status = 'pending'
                 ORDER BY node_id LIMIT $1",
            )
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        AttachmentReprocessScope::ExtractorVersion(version) => {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT DISTINCT node_id FROM email_attachment_artifacts
                 WHERE state = 'ready'
                   AND text_status = 'completed'
                   AND text_extractor_version IS DISTINCT FROM $1
                 ORDER BY node_id LIMIT $2",
            )
            .bind(version)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
    };

    if !matches!(scope, AttachmentReprocessScope::Part { .. }) {
        // Reset the selected messages so the handler treats them as work.
        sqlx::query(
            "UPDATE email_attachment_artifacts SET text_status = 'pending', updated_at = now()
             WHERE node_id = ANY($1) AND state = 'ready'",
        )
        .bind(&nodes)
        .execute(pool)
        .await?;
    }

    let mut enqueued = 0;
    for node_id in nodes {
        if enqueue_job_with_context(
            pool,
            JobType::AttachmentExtraction,
            node_id,
            20,
            JobOrigin::Repair,
            None,
            default_resource_class(JobType::AttachmentExtraction),
        )
        .await?
        .is_some()
        {
            enqueued += 1;
        }
    }
    Ok(enqueued)
}

/// One ranked email search hit.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct EmailSearchResult {
    pub node_id: Uuid,
    pub subject: Option<String>,
    pub sent_at: Option<DateTime<Utc>>,
    pub snippet: String,
    pub attachment_count: i32,
    pub duplicate_count: i64,
    pub thread_count: i64,
    pub score: f32,
    /// Primary `From` address, absent when the message declared none.
    pub from_address: Option<String>,
    pub from_display_name: Option<String>,
    pub labels: Vec<String>,
    /// Which parts of the message the query matched, so a result can explain
    /// itself rather than appearing for no visible reason. Empty for a
    /// filter-only search, which has no text to attribute.
    pub match_sources: Vec<String>,
    /// Attachment filename and page for the best attachment-content match.
    pub matched_attachment: Option<String>,
    pub matched_attachment_page: Option<i32>,
}

/// Structured narrowing applied alongside (or instead of) a text query.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmailSearchFilters {
    pub from: Vec<String>,
    pub participant: Vec<String>,
    pub labels: Vec<String>,
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    pub has_attachment: Option<bool>,
    pub status: Option<EmailExtractionStatus>,
    pub thread_group_id: Option<Uuid>,
    pub duplicate_group_id: Option<Uuid>,
    pub include_trashed: bool,
    pub include_duplicates: bool,
}

impl EmailSearchFilters {
    /// Whether any structured narrowing is present.
    ///
    /// An entirely unconstrained request — no query and no filter — must be
    /// rejected rather than allowed to page the whole archive.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.from.is_empty()
            && self.participant.is_empty()
            && self.labels.is_empty()
            && self.after.is_none()
            && self.before.is_none()
            && self.has_attachment.is_none()
            && self.status.is_none()
            && self.thread_group_id.is_none()
            && self.duplicate_group_id.is_none()
    }
}

/// Stable position in a result page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmailSearchCursor {
    pub score: f32,
    pub sent_at: Option<DateTime<Utc>>,
    pub node_id: Uuid,
}

/// Populates missing search vectors in one bounded batch.
///
/// Returns the number of rows updated. Migration adds the column empty and
/// this runs afterwards, so no deployment blocks on an archive-wide rewrite.
///
/// # Errors
///
/// Returns a database error when the batch cannot be updated.
pub async fn backfill_email_search_vectors(pool: &PgPool, limit: u32) -> Result<u64, sqlx::Error> {
    let limit = i64::from(limit.clamp(1, 10_000));
    let updated = sqlx::query(
        r"
        UPDATE email_messages SET updated_at = updated_at
        WHERE node_id IN (
            SELECT node_id FROM email_messages
            WHERE search_vector IS NULL
            ORDER BY node_id
            LIMIT $1
        )
        ",
    )
    .bind(limit)
    .execute(pool)
    .await?;
    Ok(updated.rows_affected())
}

/// Counts messages still awaiting a search vector.
///
/// # Errors
///
/// Returns a database error when the count cannot be read.
pub async fn count_email_messages_without_search_vector(pool: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM email_messages WHERE search_vector IS NULL")
        .fetch_one(pool)
        .await
}

/// Runs a weighted, filtered, cursor-paginated email search.
///
/// Ranking uses cover density (`ts_rank_cd`) so proximity matters, with a
/// deterministic `(score, sent_at, node_id)` tie-break that keeps deep paging
/// stable across equal scores.
///
/// # Errors
///
/// Returns a database error when the search query cannot run.
#[allow(clippy::too_many_lines)]
pub async fn search_email(
    pool: &PgPool,
    query: Option<&str>,
    filters: &EmailSearchFilters,
    cursor: Option<EmailSearchCursor>,
    limit: u32,
) -> Result<Vec<EmailSearchResult>, sqlx::Error> {
    let limit = i64::from(limit.clamp(1, 100));
    sqlx::query_as::<_, EmailSearchResult>(
        r"
        WITH matched AS (
            SELECT e.node_id,
                   e.subject,
                   e.sent_at,
                   e.attachment_count,
                   e.duplicate_group_id,
                   e.thread_group_id,
                   CASE
                       WHEN $1::text IS NULL THEN 0::real
                       ELSE ts_rank_cd(e.search_vector,
                                       websearch_to_tsquery('english', $1)
                                       || websearch_to_tsquery('simple', $1))
                   END AS score,
                   CASE
                       WHEN $1::text IS NULL THEN left(e.preview_text, 240)
                       ELSE ts_headline('english', e.body_text,
                                        websearch_to_tsquery('english', $1),
                                        'StartSel=[[,StopSel=]],MaxFragments=2,MaxWords=28,MinWords=8')
                   END AS snippet
            FROM email_messages e
            JOIN nodes n ON n.id = e.node_id
            -- Prose is indexed stemmed while addresses, labels, and filenames
            -- are indexed verbatim, so the query is asked in both
            -- configurations. Asking only in english stems a label such as
            -- Receipts to receipt, which then matches nothing.
            WHERE ($1::text IS NULL
                   OR e.search_vector @@ (websearch_to_tsquery('english', $1)
                                          || websearch_to_tsquery('simple', $1)))
              AND ($2::boolean OR n.lifecycle_state = 'active')
              AND ($3::text[] IS NULL OR EXISTS (
                    SELECT 1 FROM email_addresses a
                    WHERE a.node_id = e.node_id AND a.address = ANY($3)
                      AND a.role IN ('from', 'sender', 'reply_to')))
              AND ($4::text[] IS NULL OR EXISTS (
                    SELECT 1 FROM email_addresses a
                    WHERE a.node_id = e.node_id AND a.address = ANY($4)))
              AND ($5::text[] IS NULL OR EXISTS (
                    SELECT 1 FROM email_labels l
                    WHERE l.node_id = e.node_id AND l.label = ANY($5)))
              AND ($6::timestamptz IS NULL OR e.sent_at >= $6)
              AND ($7::timestamptz IS NULL OR e.sent_at < $7)
              AND ($8::boolean IS NULL
                   OR ($8 AND e.attachment_count > 0)
                   OR (NOT $8 AND e.attachment_count = 0))
              AND ($9::email_extraction_status IS NULL OR e.status = $9)
              AND ($10::uuid IS NULL OR e.thread_group_id = $10)
              AND ($11::uuid IS NULL OR e.duplicate_group_id = $11)
        ),
        counted AS (
            SELECT m.*,
                   CASE WHEN m.duplicate_group_id IS NULL THEN 1
                        ELSE count(*) OVER (PARTITION BY m.duplicate_group_id)
                   END AS duplicate_count,
                   CASE WHEN m.thread_group_id IS NULL THEN 1
                        ELSE count(*) OVER (PARTITION BY m.thread_group_id)
                   END AS thread_count,
                   -- Collapsing picks a deterministic representative so the
                   -- same copy is chosen on every run.
                   row_number() OVER (
                       PARTITION BY coalesce(m.duplicate_group_id, m.node_id)
                       ORDER BY m.score DESC, m.sent_at DESC NULLS LAST, m.node_id
                   ) AS duplicate_rank
            FROM matched m
        ),
        page AS (
            SELECT node_id, subject, sent_at, snippet, attachment_count,
                   duplicate_count, thread_count, score
            FROM counted
            WHERE ($12::boolean OR duplicate_rank = 1)
              -- Every ordering term is descending so one row-wise comparison can
              -- express the whole cursor. Mixing a descending score with an
              -- ascending id would make this test the wrong side of the tie and
              -- return rows the previous page already delivered.
              AND ($13::real IS NULL
                   OR (score, coalesce(sent_at, '-infinity'::timestamptz), node_id)
                       < ($13, coalesce($14::timestamptz, '-infinity'::timestamptz), $15::uuid))
            ORDER BY score DESC, sent_at DESC NULLS LAST, node_id DESC
            LIMIT $16
        )
        -- Sender, labels, and match provenance are joined after the page is cut,
        -- so these lookups run once per returned row rather than once per match.
        SELECT p.*,
               sender.address AS from_address,
               sender.display_name AS from_display_name,
               coalesce(labels.values, ARRAY[]::text[]) AS labels,
               -- A result that cannot say why it matched is hard to trust, and
               -- an attachment-only hit looks like a mistake without it.
               array_remove(ARRAY[
                   CASE WHEN provenance.subject_match THEN 'subject' END,
                   CASE WHEN provenance.address_match THEN 'headers' END,
                   CASE WHEN provenance.body_match THEN 'body' END,
                   CASE WHEN provenance.filename_match THEN 'attachment_filename' END,
                   CASE WHEN attachment.filename IS NOT NULL
                         OR attachment.page_number IS NOT NULL THEN 'attachment_content' END
               ], NULL) AS match_sources,
               attachment.filename AS matched_attachment,
               attachment.page_number AS matched_attachment_page
        FROM page p
        LEFT JOIN LATERAL (
            SELECT a.address, a.display_name
            FROM email_addresses a
            WHERE a.node_id = p.node_id AND a.role = 'from'
            ORDER BY a.position
            LIMIT 1
        ) sender ON TRUE
        LEFT JOIN LATERAL (
            SELECT array_agg(l.label ORDER BY l.label) AS values
            FROM email_labels l
            WHERE l.node_id = p.node_id
        ) labels ON TRUE
        LEFT JOIN LATERAL (
            SELECT
                $1::text IS NOT NULL AND (
                    to_tsvector('english', coalesce(p.subject, ''))
                    || to_tsvector('simple', coalesce(p.subject, ''))
                ) @@ (websearch_to_tsquery('english', $1)
                      || websearch_to_tsquery('simple', $1)) AS subject_match,
                $1::text IS NOT NULL AND EXISTS (
                    SELECT 1 FROM email_addresses a
                    WHERE a.node_id = p.node_id
                      AND to_tsvector('simple',
                              coalesce(a.display_name, '') || ' ' || a.address)
                          @@ websearch_to_tsquery('simple', $1)
                ) AS address_match,
                -- Body is tested against the stored vector's weight-C class
                -- rather than by re-tokenizing the body, which can be megabytes.
                -- The weight array is ordered {D,C,B,A}.
                $1::text IS NOT NULL AND ts_rank('{0,1,0,0}', e.search_vector,
                    websearch_to_tsquery('english', $1)
                    || websearch_to_tsquery('simple', $1)) > 0 AS body_match,
                $1::text IS NOT NULL AND EXISTS (
                    SELECT 1 FROM email_attachments ea
                    WHERE ea.node_id = p.node_id AND ea.filename IS NOT NULL
                      AND to_tsvector('simple', ea.filename)
                          @@ websearch_to_tsquery('simple', $1)
                ) AS filename_match
            FROM email_messages e
            WHERE e.node_id = p.node_id
        ) provenance ON TRUE
        LEFT JOIN LATERAL (
            SELECT ea.filename, t.page_number
            FROM email_attachment_text t
            LEFT JOIN email_attachments ea
                   ON ea.node_id = t.node_id AND ea.part_path = t.part_path
            WHERE t.node_id = p.node_id
              AND $1::text IS NOT NULL
              AND to_tsvector('english', t.content)
                  @@ websearch_to_tsquery('english', $1)
            ORDER BY t.part_path, t.page_number
            LIMIT 1
        ) attachment ON TRUE
        ORDER BY p.score DESC, p.sent_at DESC NULLS LAST, p.node_id DESC
        ",
    )
    .bind(query)
    .bind(filters.include_trashed)
    .bind(none_if_empty(&filters.from))
    .bind(none_if_empty(&filters.participant))
    .bind(none_if_empty(&filters.labels))
    .bind(filters.after)
    .bind(filters.before)
    .bind(filters.has_attachment)
    .bind(filters.status)
    .bind(filters.thread_group_id)
    .bind(filters.duplicate_group_id)
    .bind(filters.include_duplicates)
    .bind(cursor.map(|value| value.score))
    .bind(cursor.and_then(|value| value.sent_at))
    .bind(cursor.map(|value| value.node_id))
    .bind(limit)
    .fetch_all(pool)
    .await
}

fn none_if_empty(values: &[String]) -> Option<&[String]> {
    (!values.is_empty()).then_some(values)
}

/// One facet bucket.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct EmailFacet {
    pub value: String,
    pub count: i64,
}

/// Bounded label facets over active messages.
///
/// # Errors
///
/// Returns a database error when the aggregate cannot run.
pub async fn email_label_facets(pool: &PgPool, limit: i64) -> Result<Vec<EmailFacet>, sqlx::Error> {
    sqlx::query_as(
        r"
        SELECT l.label AS value, count(*) AS count
        FROM email_labels l
        JOIN nodes n ON n.id = l.node_id AND n.lifecycle_state = 'active'
        GROUP BY l.label
        ORDER BY count DESC, l.label
        LIMIT $1
        ",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Bounded correspondent facets over active messages.
///
/// # Errors
///
/// Returns a database error when the aggregate cannot run.
pub async fn email_correspondent_facets(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<EmailFacet>, sqlx::Error> {
    sqlx::query_as(
        r"
        SELECT a.address AS value, count(*) AS count
        FROM email_addresses a
        JOIN nodes n ON n.id = a.node_id AND n.lifecycle_state = 'active'
        WHERE a.role IN ('from', 'sender', 'reply_to')
        GROUP BY a.address
        ORDER BY count DESC, a.address
        LIMIT $1
        ",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Message counts per calendar year, newest first.
///
/// # Errors
///
/// Returns a database error when the aggregate cannot run.
pub async fn email_year_facets(pool: &PgPool) -> Result<Vec<EmailFacet>, sqlx::Error> {
    sqlx::query_as(
        r"
        SELECT to_char(date_trunc('year', e.sent_at), 'YYYY') AS value,
               count(*) AS count
        FROM email_messages e
        JOIN nodes n ON n.id = e.node_id AND n.lifecycle_state = 'active'
        WHERE e.sent_at IS NOT NULL
        GROUP BY 1
        ORDER BY 1 DESC
        ",
    )
    .fetch_all(pool)
    .await
}

/// Read-only projection of the historical email workload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmailPreflightReport {
    pub snapshot_before: DateTime<Utc>,
    pub parser_version: Option<String>,
    pub candidates: i64,
    pub already_completed: i64,
    pub already_failed: i64,
    pub already_skipped: i64,
    pub already_unsupported: i64,
    pub total_candidate_bytes: i64,
    pub p50_bytes: i64,
    pub p95_bytes: i64,
    pub max_bytes: i64,
}

/// Email scopes accepted by bounded manual reprocessing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmailReprocessScope {
    Node(Uuid),
    Failed,
    Missing,
    VersionMismatch(String),
}

// A historical email candidate is an active finalized file with no email
// projection, or one produced by a different parser version. Unlike OCR,
// candidacy does not require `node_metadata`: `.eml` files are frequently
// detected as `text/plain`, so filtering on detected MIME here would hide most
// of the archive. The handler confirms the RFC 5322 shape from bytes and
// records `unsupported` for anything else, which costs one cheap sniff per
// non-email file and is recoverable, where a wrong pre-filter is silent.

/// Builds the read-only email preflight report for a snapshot boundary.
///
/// # Errors
///
/// Returns a database error when an aggregate query cannot run.
pub async fn email_preflight_report(
    pool: &PgPool,
    snapshot_before: DateTime<Utc>,
    parser_version: Option<&str>,
) -> Result<EmailPreflightReport, sqlx::Error> {
    let sizes = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        r"
        SELECT count(*) AS candidates,
               COALESCE(sum(f.byte_size), 0)::bigint AS total_bytes,
               COALESCE(percentile_disc(0.5) WITHIN GROUP (ORDER BY f.byte_size), 0)::bigint
                   AS p50_bytes,
               COALESCE(percentile_disc(0.95) WITHIN GROUP (ORDER BY f.byte_size), 0)::bigint
                   AS p95_bytes,
               COALESCE(max(f.byte_size), 0)::bigint AS max_bytes
        FROM nodes n
        JOIN file_objects f ON f.node_id = n.id AND f.upload_state = 'finalized'
        LEFT JOIN email_messages e ON e.node_id = n.id
        WHERE n.kind = 'file' AND n.lifecycle_state = 'active'
          AND n.created_at < $1
          AND (e.node_id IS NULL OR e.parser_version IS DISTINCT FROM $2)
        ",
    )
    .bind(snapshot_before)
    .bind(parser_version)
    .fetch_one(pool)
    .await?;

    let outcomes = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        r"
        SELECT
            count(*) FILTER (WHERE status = 'completed') AS completed,
            count(*) FILTER (WHERE status = 'failed') AS failed,
            count(*) FILTER (WHERE status = 'skipped') AS skipped,
            count(*) FILTER (WHERE status = 'unsupported') AS unsupported
        FROM email_messages
        ",
    )
    .fetch_one(pool)
    .await?;

    Ok(EmailPreflightReport {
        snapshot_before,
        parser_version: parser_version.map(ToOwned::to_owned),
        candidates: sizes.0,
        already_completed: outcomes.0,
        already_failed: outcomes.1,
        already_skipped: outcomes.2,
        already_unsupported: outcomes.3,
        total_candidate_bytes: sizes.1,
        p50_bytes: sizes.2,
        p95_bytes: sizes.3,
        max_bytes: sizes.4,
    })
}

/// Enqueues one bounded historical email batch and advances the cursor.
///
/// Selection, enqueue, cursor advance, and the aggregate count share one
/// transaction, so an interrupted refill can neither skip nor repeat a file.
///
/// # Errors
///
/// Returns a database error when the bounded refill transaction cannot commit.
pub async fn enqueue_email_backfill_batch(
    pool: &PgPool,
    campaign: &BackfillCampaignRecord,
    parser_version: Option<&str>,
    allowance: i64,
) -> Result<(u64, bool), sqlx::Error> {
    let Some(snapshot_before) = campaign.snapshot_before else {
        return Ok((0, false));
    };
    if allowance <= 0 {
        return Ok((0, false));
    }
    let mut transaction = pool.begin().await?;
    // The campaign state is re-read inside the transaction. Only the cursor
    // update was previously guarded, so a paused campaign could still enqueue
    // work while its cursor stood still — leaving those nodes with active jobs
    // that a later resume would skip. The scheduler only calls this for running
    // campaigns, but the function must be safe for any caller.
    let state = sqlx::query_scalar::<_, BackfillState>(
        "SELECT state FROM backfill_campaigns WHERE id = $1 FOR UPDATE",
    )
    .bind(campaign.id)
    .fetch_optional(&mut *transaction)
    .await?;
    if state != Some(BackfillState::Running) {
        transaction.commit().await?;
        return Ok((0, false));
    }
    let candidates = sqlx::query_as::<_, (Uuid, DateTime<Utc>)>(
        r"
        SELECT n.id, n.created_at
        FROM nodes n
        JOIN file_objects f ON f.node_id = n.id AND f.upload_state = 'finalized'
        LEFT JOIN email_messages e ON e.node_id = n.id
        WHERE n.kind = 'file' AND n.lifecycle_state = 'active'
          AND n.created_at < $1
          AND (e.node_id IS NULL OR e.parser_version IS DISTINCT FROM $2)
          AND ($3::timestamptz IS NULL OR (n.created_at, n.id) > ($3, $4))
          AND NOT EXISTS (
              SELECT 1 FROM jobs j
              WHERE j.target_node_id = n.id AND j.job_type = 'email_extraction'
                AND j.state IN ('pending', 'leased')
          )
        ORDER BY n.created_at, n.id
        LIMIT $5
        ",
    )
    .bind(snapshot_before)
    .bind(parser_version)
    .bind(campaign.cursor_created_at)
    .bind(campaign.cursor_node_id)
    .bind(allowance)
    .fetch_all(&mut *transaction)
    .await?;

    let exhausted = i64::try_from(candidates.len()).unwrap_or(i64::MAX) < allowance;
    let Some(&(last_id, last_created_at)) = candidates.last() else {
        transaction.commit().await?;
        return Ok((0, true));
    };

    let mut enqueued = 0u64;
    for (node_id, _) in &candidates {
        let inserted = sqlx::query(
            r"
            INSERT INTO jobs
                (id, job_type, target_node_id, priority, origin, campaign_id, resource_class)
            VALUES ($1, 'email_extraction', $2, -100, 'backfill', $3, 'heavy_cpu')
            ON CONFLICT DO NOTHING
            ",
        )
        .bind(Uuid::new_v4())
        .bind(node_id)
        .bind(campaign.id)
        .execute(&mut *transaction)
        .await?;
        enqueued += inserted.rows_affected();
    }

    sqlx::query(
        r"
        UPDATE backfill_campaigns
        SET enqueued_count = enqueued_count + $2, cursor_created_at = $3,
            cursor_node_id = $4, updated_at = now()
        WHERE id = $1 AND state = 'running'
        ",
    )
    .bind(campaign.id)
    .bind(i64::try_from(enqueued).unwrap_or(i64::MAX))
    .bind(last_created_at)
    .bind(last_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok((enqueued, exhausted))
}

/// Enqueues a bounded set of manual email jobs, ignoring already-active work.
///
/// # Errors
///
/// Returns a database error when candidates or the queue cannot be accessed.
pub async fn enqueue_email_reprocessing(
    pool: &PgPool,
    scope: &EmailReprocessScope,
    limit: u32,
) -> Result<u64, sqlx::Error> {
    let limit = i64::from(limit.clamp(1, 100));
    // Active work is excluded inside each candidate query, before the limit is
    // applied. Filtering afterwards would let a batch report zero while
    // eligible nodes waited further down the result set.
    let node_ids = match scope {
        EmailReprocessScope::Node(node_id) => {
            sqlx::query_scalar::<_, Uuid>(
                r"
                SELECT n.id FROM nodes n
                JOIN file_objects f ON f.node_id = n.id AND f.upload_state = 'finalized'
                WHERE n.id = $1 AND n.kind = 'file' AND n.lifecycle_state = 'active'
                  AND NOT EXISTS (
                      SELECT 1 FROM jobs j
                      WHERE j.target_node_id = n.id AND j.job_type = 'email_extraction'
                        AND j.state IN ('pending', 'leased')
                  )
                LIMIT 1
                ",
            )
            .bind(node_id)
            .fetch_all(pool)
            .await?
        }
        EmailReprocessScope::Failed => {
            sqlx::query_scalar::<_, Uuid>(
                r"
                SELECT n.id FROM nodes n
                JOIN email_messages e ON e.node_id = n.id
                WHERE n.lifecycle_state = 'active' AND e.status = 'failed'
                  AND NOT EXISTS (
                      SELECT 1 FROM jobs j
                      WHERE j.target_node_id = n.id AND j.job_type = 'email_extraction'
                        AND j.state IN ('pending', 'leased')
                  )
                ORDER BY e.updated_at, n.id
                LIMIT $1
                ",
            )
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        EmailReprocessScope::Missing => {
            sqlx::query_scalar::<_, Uuid>(
                r"
                SELECT n.id FROM nodes n
                JOIN file_objects f ON f.node_id = n.id AND f.upload_state = 'finalized'
                LEFT JOIN email_messages e ON e.node_id = n.id
                WHERE n.kind = 'file' AND n.lifecycle_state = 'active' AND e.node_id IS NULL
                  AND NOT EXISTS (
                      SELECT 1 FROM jobs j
                      WHERE j.target_node_id = n.id AND j.job_type = 'email_extraction'
                        AND j.state IN ('pending', 'leased')
                  )
                ORDER BY n.created_at, n.id
                LIMIT $1
                ",
            )
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        EmailReprocessScope::VersionMismatch(version) => {
            sqlx::query_scalar::<_, Uuid>(
                r"
                SELECT n.id FROM nodes n
                JOIN email_messages e ON e.node_id = n.id
                WHERE n.lifecycle_state = 'active' AND e.parser_version <> $1
                  AND NOT EXISTS (
                      SELECT 1 FROM jobs j
                      WHERE j.target_node_id = n.id AND j.job_type = 'email_extraction'
                        AND j.state IN ('pending', 'leased')
                  )
                ORDER BY e.updated_at, n.id
                LIMIT $2
                ",
            )
            .bind(version)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
    };
    let mut enqueued = 0;
    for node_id in node_ids {
        if enqueue_job_with_context(
            pool,
            JobType::EmailExtraction,
            node_id,
            -50,
            JobOrigin::Repair,
            None,
            JobResourceClass::HeavyCpu,
        )
        .await?
        .is_some()
        {
            enqueued += 1;
        }
    }
    Ok(enqueued)
}

/// One MIME family bucket in the read-only OCR preflight report.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct OcrPreflightFamily {
    pub detected_mime: String,
    pub candidates: i64,
    pub total_bytes: i64,
    pub p50_bytes: i64,
    pub p95_bytes: i64,
    pub max_bytes: i64,
}

/// Read-only projection of the historical OCR workload.
///
/// Every field is derived from indexed aggregates over already-extracted
/// metadata. Nothing here opens a managed original or enqueues a job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OcrPreflightReport {
    pub snapshot_before: DateTime<Utc>,
    pub engine_version: Option<String>,
    pub candidates: i64,
    pub already_completed: i64,
    pub already_skipped: i64,
    pub already_failed: i64,
    pub already_unsupported: i64,
    pub awaiting_metadata: i64,
    pub total_candidate_bytes: i64,
    pub families: Vec<OcrPreflightFamily>,
}

// A node is a historical OCR candidate when it is an active finalized file
// whose detected MIME is an OCR input and which has no document text for the
// current engine version. Files whose metadata job has not yet run have no
// `detected_mime` and are deliberately excluded: they are counted separately as
// `awaiting_metadata` rather than guessed at from their filename. The predicate
// is repeated verbatim in the two queries below because sqlx only accepts
// `&'static str`; the duplication is covered by `ocr_backfill_candidates.rs`.

/// Builds the read-only OCR preflight report for a snapshot boundary.
///
/// # Errors
///
/// Returns a database error when an aggregate query cannot run.
pub async fn ocr_preflight_report(
    pool: &PgPool,
    supported_mimes: &[String],
    snapshot_before: DateTime<Utc>,
    engine_version: Option<&str>,
) -> Result<OcrPreflightReport, sqlx::Error> {
    let families = sqlx::query_as::<_, OcrPreflightFamily>(
        r"
        SELECT m.detected_mime,
               count(*) AS candidates,
               COALESCE(sum(f.byte_size), 0)::bigint AS total_bytes,
               COALESCE(percentile_disc(0.5) WITHIN GROUP (ORDER BY f.byte_size), 0)::bigint
                   AS p50_bytes,
               COALESCE(percentile_disc(0.95) WITHIN GROUP (ORDER BY f.byte_size), 0)::bigint
                   AS p95_bytes,
               COALESCE(max(f.byte_size), 0)::bigint AS max_bytes
        FROM nodes n
        JOIN file_objects f ON f.node_id = n.id
        JOIN node_metadata m ON m.node_id = n.id
        LEFT JOIN document_text d ON d.node_id = n.id
        WHERE n.kind = 'file'
          AND n.lifecycle_state = 'active'
          AND f.upload_state = 'finalized'
          AND m.detected_mime = ANY($1)
          AND n.created_at < $2
          AND (d.node_id IS NULL
               OR (d.engine_version IS DISTINCT FROM $3 AND d.status <> 'skipped'))
        GROUP BY m.detected_mime
        ORDER BY candidates DESC, m.detected_mime
        ",
    )
    .bind(supported_mimes)
    .bind(snapshot_before)
    .bind(engine_version)
    .fetch_all(pool)
    .await?;

    let outcomes = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        r"
        SELECT
            (SELECT count(*) FROM document_text
             WHERE status = 'completed' AND source = 'ocr') AS already_completed,
            (SELECT count(*) FROM document_text
             WHERE status = 'skipped'
                OR (status = 'completed' AND source = 'embedded')) AS already_skipped,
            (SELECT count(*) FROM document_text WHERE status = 'failed') AS already_failed,
            (SELECT count(*) FROM document_text WHERE status = 'unsupported')
                AS already_unsupported,
            (SELECT count(*)
             FROM nodes n
             JOIN file_objects f ON f.node_id = n.id AND f.upload_state = 'finalized'
             LEFT JOIN node_metadata m ON m.node_id = n.id
             WHERE n.kind = 'file' AND n.lifecycle_state = 'active'
               AND n.created_at < $1 AND m.node_id IS NULL) AS awaiting_metadata
        ",
    )
    .bind(snapshot_before)
    .fetch_one(pool)
    .await?;

    let candidates = families.iter().map(|family| family.candidates).sum();
    let total_candidate_bytes = families.iter().map(|family| family.total_bytes).sum();
    Ok(OcrPreflightReport {
        snapshot_before,
        engine_version: engine_version.map(ToOwned::to_owned),
        candidates,
        already_completed: outcomes.0,
        already_skipped: outcomes.1,
        already_failed: outcomes.2,
        already_unsupported: outcomes.3,
        awaiting_metadata: outcomes.4,
        total_candidate_bytes,
        families,
    })
}

/// Enqueues one bounded historical OCR batch and advances the campaign cursor.
///
/// Candidate ordering, the enqueue, the cursor advance, and the aggregate count
/// share one transaction, so an interrupted refill either advances fully or not
/// at all and can never skip a candidate. Returns the number enqueued and
/// whether the campaign has reached the end of its candidate set.
///
/// # Errors
///
/// Returns a database error when the bounded refill transaction cannot commit.
pub async fn enqueue_ocr_backfill_batch(
    pool: &PgPool,
    campaign: &BackfillCampaignRecord,
    supported_mimes: &[String],
    engine_version: Option<&str>,
    allowance: i64,
) -> Result<(u64, bool), sqlx::Error> {
    let Some(snapshot_before) = campaign.snapshot_before else {
        // An unprepared campaign has no frozen boundary and must not enumerate.
        return Ok((0, false));
    };
    if allowance <= 0 {
        return Ok((0, false));
    }
    let mut transaction = pool.begin().await?;
    // The campaign state is re-read inside the transaction. Only the cursor
    // update was previously guarded, so a paused campaign could still enqueue
    // work while its cursor stood still — leaving those nodes with active jobs
    // that a later resume would skip. The scheduler only calls this for running
    // campaigns, but the function must be safe for any caller.
    let state = sqlx::query_scalar::<_, BackfillState>(
        "SELECT state FROM backfill_campaigns WHERE id = $1 FOR UPDATE",
    )
    .bind(campaign.id)
    .fetch_optional(&mut *transaction)
    .await?;
    if state != Some(BackfillState::Running) {
        transaction.commit().await?;
        return Ok((0, false));
    }
    let candidates = sqlx::query_as::<_, (Uuid, DateTime<Utc>)>(
        r"
        SELECT n.id, n.created_at
        FROM nodes n
        JOIN file_objects f ON f.node_id = n.id
        JOIN node_metadata m ON m.node_id = n.id
        LEFT JOIN document_text d ON d.node_id = n.id
        WHERE n.kind = 'file'
          AND n.lifecycle_state = 'active'
          AND f.upload_state = 'finalized'
          AND m.detected_mime = ANY($1)
          AND n.created_at < $2
          AND (d.node_id IS NULL
               OR (d.engine_version IS DISTINCT FROM $3 AND d.status <> 'skipped'))
          AND ($4::timestamptz IS NULL OR (n.created_at, n.id) > ($4, $5))
          AND NOT EXISTS (
              SELECT 1 FROM jobs j
              WHERE j.target_node_id = n.id AND j.job_type = 'ocr'
                AND j.state IN ('pending', 'leased')
          )
        ORDER BY n.created_at, n.id
        LIMIT $6
        ",
    )
    .bind(supported_mimes)
    .bind(snapshot_before)
    .bind(engine_version)
    .bind(campaign.cursor_created_at)
    .bind(campaign.cursor_node_id)
    .bind(allowance)
    .fetch_all(&mut *transaction)
    .await?;

    let exhausted = i64::try_from(candidates.len()).unwrap_or(i64::MAX) < allowance;
    let Some(&(last_id, last_created_at)) = candidates.last() else {
        transaction.commit().await?;
        return Ok((0, true));
    };

    let mut enqueued = 0u64;
    for (node_id, _) in &candidates {
        let inserted = sqlx::query(
            r"
            INSERT INTO jobs
                (id, job_type, target_node_id, priority, origin, campaign_id, resource_class)
            VALUES ($1, 'ocr', $2, -100, 'backfill', $3, 'heavy_cpu')
            ON CONFLICT DO NOTHING
            ",
        )
        .bind(Uuid::new_v4())
        .bind(node_id)
        .bind(campaign.id)
        .execute(&mut *transaction)
        .await?;
        enqueued += inserted.rows_affected();
    }

    sqlx::query(
        r"
        UPDATE backfill_campaigns
        SET enqueued_count = enqueued_count + $2, cursor_created_at = $3,
            cursor_node_id = $4, updated_at = now()
        WHERE id = $1 AND state = 'running'
        ",
    )
    .bind(campaign.id)
    .bind(i64::try_from(enqueued).unwrap_or(i64::MAX))
    .bind(last_created_at)
    .bind(last_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok((enqueued, exhausted))
}

/// One durable background job.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct JobRecord {
    pub id: Uuid,
    pub job_type: JobType,
    pub target_node_id: Uuid,
    pub import_source_id: Option<Uuid>,
    pub state: JobState,
    pub origin: JobOrigin,
    pub campaign_id: Option<Uuid>,
    pub resource_class: JobResourceClass,
    pub priority: i32,
    pub attempts: i32,
    pub max_attempts: i32,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Durable campaign configuration, cursor, and aggregate progress.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct BackfillCampaignRecord {
    pub id: Uuid,
    pub kind: BackfillKind,
    pub state: BackfillState,
    pub candidate_definition: serde_json::Value,
    pub snapshot_before: Option<DateTime<Utc>>,
    pub cursor_created_at: Option<DateTime<Utc>>,
    pub cursor_node_id: Option<Uuid>,
    pub batch_size: i32,
    pub max_queued: i32,
    pub max_running: i32,
    pub resource_class: JobResourceClass,
    pub foreground_fairness: i32,
    pub candidate_count: i64,
    pub enqueued_count: i64,
    pub completed_count: i64,
    pub failed_count: i64,
    pub skipped_count: i64,
    pub created_by_version: String,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub paused_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Append-only campaign audit entry, also used as an SSE cursor.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct BackfillCampaignEventRecord {
    pub id: i64,
    pub campaign_id: Uuid,
    pub old_state: Option<BackfillState>,
    pub new_state: Option<BackfillState>,
    pub event_type: String,
    pub reason: Option<String>,
    pub details: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Low-water information consumed by a kind-specific candidate provider.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct BackfillRefillWindow {
    pub campaign_id: Uuid,
    pub batch_size: i32,
    pub queued: i64,
    pub allowance: i64,
}

/// Live queue and recent completion measurements for an operator campaign.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct BackfillCampaignMetrics {
    pub campaign_id: Uuid,
    pub pending: i64,
    pub running: i64,
    pub completed_last_hour: i64,
    pub observed_seconds: f64,
}

/// Validated settings used to create an inert draft campaign.
#[derive(Clone, Debug, PartialEq)]
pub struct NewBackfillCampaign {
    pub kind: BackfillKind,
    pub candidate_definition: serde_json::Value,
    pub batch_size: i32,
    pub max_queued: i32,
    pub max_running: i32,
    pub resource_class: JobResourceClass,
    pub foreground_fairness: i32,
    pub created_by_version: String,
}

/// Creates a draft campaign and its first audit event.
///
/// # Errors
///
/// Returns a database error when the campaign or event cannot be persisted.
pub async fn create_backfill_campaign(
    pool: &PgPool,
    request: &NewBackfillCampaign,
) -> Result<BackfillCampaignRecord, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let campaign = sqlx::query_as::<_, BackfillCampaignRecord>(
        r"
        INSERT INTO backfill_campaigns (
            id, kind, candidate_definition, batch_size, max_queued, max_running,
            resource_class, foreground_fairness, created_by_version
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING *
        ",
    )
    .bind(Uuid::new_v4())
    .bind(request.kind)
    .bind(&request.candidate_definition)
    .bind(request.batch_size)
    .bind(request.max_queued)
    .bind(request.max_running)
    .bind(request.resource_class)
    .bind(request.foreground_fairness)
    .bind(&request.created_by_version)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query(
        r"
        INSERT INTO backfill_campaign_events
            (campaign_id, new_state, event_type, details)
        VALUES ($1, 'draft', 'created', $2)
        ",
    )
    .bind(campaign.id)
    .bind(&request.candidate_definition)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(campaign)
}

/// Returns all campaigns newest first.
///
/// # Errors
///
/// Returns a database error when campaigns cannot be queried.
pub async fn list_backfill_campaigns(
    pool: &PgPool,
) -> Result<Vec<BackfillCampaignRecord>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM backfill_campaigns ORDER BY created_at DESC, id DESC")
        .fetch_all(pool)
        .await
}

/// Fetches one campaign.
///
/// # Errors
///
/// Returns a database error when the campaign cannot be queried.
pub async fn get_backfill_campaign(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<BackfillCampaignRecord>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM backfill_campaigns WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Records a stable candidate snapshot and leaves the campaign paused.
///
/// # Errors
///
/// Returns a database error when the campaign or audit event cannot be updated.
pub async fn prepare_backfill_campaign(
    pool: &PgPool,
    id: Uuid,
    candidate_count: i64,
    snapshot_before: DateTime<Utc>,
    reason: Option<&str>,
) -> Result<Option<BackfillCampaignRecord>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let campaign = sqlx::query_as::<_, BackfillCampaignRecord>(
        r"
        UPDATE backfill_campaigns
        SET state = 'paused', candidate_count = $2, snapshot_before = $3,
            updated_at = now()
        WHERE id = $1 AND state = 'draft' AND $2 >= 0
        RETURNING *
        ",
    )
    .bind(id)
    .bind(candidate_count)
    .bind(snapshot_before)
    .fetch_optional(&mut *transaction)
    .await?;
    if campaign.is_some() {
        sqlx::query(
            r"
            INSERT INTO backfill_campaign_events
                (campaign_id, old_state, new_state, event_type, reason,
                 details)
            VALUES ($1, 'draft', 'paused', 'prepared', $2,
                    jsonb_build_object('candidate_count', $3, 'snapshot_before', $4))
            ",
        )
        .bind(id)
        .bind(reason)
        .bind(candidate_count)
        .bind(snapshot_before)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(campaign)
}

/// Applies an allowed campaign state transition and appends an audit event.
///
/// # Errors
///
/// Returns a database error when the transition cannot be applied atomically.
pub async fn transition_backfill_campaign(
    pool: &PgPool,
    id: Uuid,
    target: BackfillState,
    reason: Option<&str>,
) -> Result<Option<BackfillCampaignRecord>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let current = sqlx::query_scalar::<_, BackfillState>(
        "SELECT state FROM backfill_campaigns WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(current) = current else {
        transaction.commit().await?;
        return Ok(None);
    };
    let allowed = matches!(
        (current, target),
        (BackfillState::Paused, BackfillState::Running)
            | (
                BackfillState::Running,
                BackfillState::Paused | BackfillState::Draining
            )
            | (
                BackfillState::Paused | BackfillState::Running | BackfillState::Draining,
                BackfillState::Cancelled | BackfillState::Failed
            )
            | (BackfillState::Draining, BackfillState::Completed)
    );
    if !allowed {
        transaction.commit().await?;
        return Ok(None);
    }
    if target == BackfillState::Running {
        // Email, OCR, and attachment backfills share one `heavy_cpu` admission
        // permit, so a second active heavy campaign could not make progress
        // anyway — it would only compete for refills and make the queue harder
        // to reason about. Refuse the transition outright instead.
        let competing = sqlx::query_scalar::<_, i64>(
            r"
            SELECT count(*) FROM backfill_campaigns
            WHERE id <> $1 AND resource_class = 'heavy_cpu'
              AND state IN ('running', 'draining')
            ",
        )
        .bind(id)
        .fetch_one(&mut *transaction)
        .await?;
        if competing > 0 {
            transaction.commit().await?;
            return Ok(None);
        }
    }
    let _campaign = sqlx::query_as::<_, BackfillCampaignRecord>(
        r"
        UPDATE backfill_campaigns
        SET state = $2, updated_at = now(),
            started_at = CASE WHEN $2 = 'running' AND started_at IS NULL THEN now() ELSE started_at END,
            paused_at = CASE WHEN $2 = 'paused' THEN now() ELSE paused_at END,
            completed_at = CASE WHEN $2 IN ('completed', 'cancelled', 'failed') THEN now() ELSE completed_at END,
            last_error = CASE WHEN $2 = 'failed' THEN $3 ELSE last_error END
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(id)
    .bind(target)
    .bind(reason)
    .fetch_one(&mut *transaction)
    .await?;
    if target == BackfillState::Cancelled {
        sqlx::query(
            r"
            UPDATE jobs
            SET state = 'skipped', last_error = 'campaign cancelled',
                completed_at = now(), updated_at = now()
            WHERE campaign_id = $1 AND state = 'pending'
            ",
        )
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query(
        r"
        INSERT INTO backfill_campaign_events
            (campaign_id, old_state, new_state, event_type, reason)
        VALUES ($1, $2, $3, 'state_changed', $4)
        ",
    )
    .bind(id)
    .bind(current)
    .bind(target)
    .bind(reason)
    .execute(&mut *transaction)
    .await?;
    let campaign = sqlx::query_as::<_, BackfillCampaignRecord>(
        "SELECT * FROM backfill_campaigns WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Some(campaign))
}

/// Returns campaign events after an SSE cursor.
///
/// # Errors
///
/// Returns a database error when campaign events cannot be queried.
pub async fn list_backfill_campaign_events_after(
    pool: &PgPool,
    campaign_id: Option<Uuid>,
    after_id: i64,
    limit: i64,
) -> Result<Vec<BackfillCampaignEventRecord>, sqlx::Error> {
    sqlx::query_as(
        r"
        SELECT * FROM backfill_campaign_events
        WHERE id > $1 AND ($2::uuid IS NULL OR campaign_id = $2)
        ORDER BY id
        LIMIT $3
        ",
    )
    .bind(after_id)
    .bind(campaign_id)
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await
}

/// Computes how many candidates a kind-specific scheduler may enqueue.
///
/// # Errors
///
/// Returns a database error when campaign queue depth cannot be queried.
pub async fn get_backfill_refill_window(
    pool: &PgPool,
    campaign_id: Uuid,
) -> Result<Option<BackfillRefillWindow>, sqlx::Error> {
    sqlx::query_as(
        r"
        SELECT c.id AS campaign_id, c.batch_size,
               count(j.id) FILTER (WHERE j.state IN ('pending', 'leased')) AS queued,
               LEAST(
                   c.batch_size::bigint,
                   GREATEST(c.max_queued::bigint - count(j.id) FILTER (
                       WHERE j.state IN ('pending', 'leased')
                   ), 0),
                   GREATEST(
                       COALESCE(
                           NULLIF(c.candidate_definition ->> 'canary_limit', '')::bigint
                               - c.enqueued_count,
                           c.batch_size::bigint
                       ),
                       0
                   )
               ) AS allowance
        FROM backfill_campaigns c
        LEFT JOIN jobs j ON j.campaign_id = c.id
        WHERE c.id = $1 AND c.state = 'running'
        GROUP BY c.id
        ",
    )
    .bind(campaign_id)
    .fetch_optional(pool)
    .await
}

/// Returns live queue depth and a bounded recent throughput window.
///
/// # Errors
///
/// Returns a database error when campaign job aggregates cannot be read.
pub async fn get_backfill_campaign_metrics(
    pool: &PgPool,
    campaign_id: Uuid,
) -> Result<Option<BackfillCampaignMetrics>, sqlx::Error> {
    sqlx::query_as(
        r"
        SELECT c.id AS campaign_id,
               count(j.id) FILTER (WHERE j.state = 'pending') AS pending,
               count(j.id) FILTER (WHERE j.state = 'leased') AS running,
               count(j.id) FILTER (
                   WHERE j.state = 'completed'
                     AND j.completed_at >= now() - interval '1 hour'
               ) AS completed_last_hour,
               COALESCE(EXTRACT(EPOCH FROM (
                   now() - min(j.completed_at) FILTER (
                       WHERE j.state = 'completed'
                         AND j.completed_at >= now() - interval '1 hour'
                   )
               ))::double precision, 0::double precision) AS observed_seconds
        FROM backfill_campaigns c
        LEFT JOIN jobs j ON j.campaign_id = c.id
        WHERE c.id = $1
        GROUP BY c.id
        ",
    )
    .bind(campaign_id)
    .fetch_optional(pool)
    .await
}

/// Records one reviewed canary result as an append-only campaign event.
///
/// A running campaign must be paused before its measurements can be approved.
///
/// # Errors
///
/// Returns a database error when the campaign or event cannot be written.
pub async fn record_backfill_canary_result(
    pool: &PgPool,
    campaign_id: Uuid,
    details: &serde_json::Value,
) -> Result<Option<BackfillCampaignEventRecord>, sqlx::Error> {
    sqlx::query_as(
        r"
        INSERT INTO backfill_campaign_events
            (campaign_id, old_state, new_state, event_type, details)
        SELECT id, state, state, 'canary_result', $2
        FROM backfill_campaigns
        WHERE id = $1 AND state IN ('paused', 'draining', 'completed')
        RETURNING *
        ",
    )
    .bind(campaign_id)
    .bind(details)
    .fetch_optional(pool)
    .await
}

/// Lists reviewed canary results in stage order and then creation order.
///
/// # Errors
///
/// Returns a database error when canary events cannot be read.
pub async fn list_backfill_canary_results(
    pool: &PgPool,
    campaign_id: Uuid,
) -> Result<Vec<BackfillCampaignEventRecord>, sqlx::Error> {
    sqlx::query_as(
        r"
        SELECT * FROM backfill_campaign_events
        WHERE campaign_id = $1 AND event_type = 'canary_result'
        ORDER BY COALESCE((details ->> 'stage')::bigint, 0), id
        ",
    )
    .bind(campaign_id)
    .fetch_all(pool)
    .await
}

/// Persists a scheduler cursor and aggregate enqueue count after one batch.
///
/// # Errors
///
/// Returns a database error when campaign progress cannot be updated.
pub async fn record_backfill_batch(
    pool: &PgPool,
    campaign_id: Uuid,
    enqueued: i64,
    cursor_created_at: Option<DateTime<Utc>>,
    cursor_node_id: Option<Uuid>,
) -> Result<Option<BackfillCampaignRecord>, sqlx::Error> {
    sqlx::query_as(
        r"
        UPDATE backfill_campaigns
        SET enqueued_count = enqueued_count + $2,
            cursor_created_at = COALESCE($3, cursor_created_at),
            cursor_node_id = COALESCE($4, cursor_node_id), updated_at = now()
        WHERE id = $1 AND state = 'running' AND $2 >= 0
        RETURNING *
        ",
    )
    .bind(campaign_id)
    .bind(enqueued)
    .bind(cursor_created_at)
    .bind(cursor_node_id)
    .fetch_optional(pool)
    .await
}

/// Enqueues work unless the target already has a pending or leased job of this type.
///
/// # Errors
///
/// Returns a database error when the job cannot be inserted.
pub async fn enqueue_job(
    pool: &PgPool,
    job_type: JobType,
    target_node_id: Uuid,
    priority: i32,
) -> Result<Option<JobRecord>, sqlx::Error> {
    enqueue_job_with_context(
        pool,
        job_type,
        target_node_id,
        priority,
        JobOrigin::Foreground,
        None,
        default_resource_class(job_type),
    )
    .await
}

/// Enqueues work with explicit provenance and capacity classification.
///
/// Backfill work must name its campaign; non-backfill work must not.
///
/// # Errors
///
/// Returns a database error when the contextual job cannot be inserted.
pub async fn enqueue_job_with_context(
    pool: &PgPool,
    job_type: JobType,
    target_node_id: Uuid,
    priority: i32,
    origin: JobOrigin,
    campaign_id: Option<Uuid>,
    resource_class: JobResourceClass,
) -> Result<Option<JobRecord>, sqlx::Error> {
    sqlx::query_as::<_, JobRecord>(
        r"
        INSERT INTO jobs (
            id, job_type, target_node_id, priority, max_attempts,
            origin, campaign_id, resource_class
        )
        VALUES (
            $1, $2, $3, $4, CASE WHEN $2 = 'ocr'::job_type THEN 5 ELSE 3 END,
            $5, $6, $7
        )
        ON CONFLICT DO NOTHING
        RETURNING *
        ",
    )
    .bind(Uuid::new_v4())
    .bind(job_type)
    .bind(target_node_id)
    .bind(priority)
    .bind(origin)
    .bind(campaign_id)
    .bind(resource_class)
    .fetch_optional(pool)
    .await
}

/// Default capacity pool for ordinary foreground jobs.
#[must_use]
pub const fn default_resource_class(job_type: JobType) -> JobResourceClass {
    match job_type {
        JobType::MetadataExtraction => JobResourceClass::Extractor,
        JobType::PreviewGeneration => JobResourceClass::Preview,
        // Email parsing starts under the shared heavy permit even though MIME
        // parsing is cheaper than OCR. ADR 0009 gates promotion to `Extractor`
        // on the 10,000-message canary in email.md Story 22.5.
        // Attachment extraction runs Tika and OCR over documents pulled out of
        // messages, so it is the same kind of sustained CPU work as OCR itself
        // and shares the heavy permit rather than competing outside it.
        JobType::Ocr | JobType::EmailExtraction | JobType::AttachmentExtraction => {
            JobResourceClass::HeavyCpu
        }
        JobType::ImportScan => JobResourceClass::HeavyIo,
        JobType::TrashCleanup | JobType::PermanentDeletion => JobResourceClass::Light,
    }
}

/// Marks a leased job skipped and records the reason it required no processing.
///
/// # Errors
///
/// Returns a database error when the job cannot be updated.
pub async fn skip_job(
    pool: &PgPool,
    job_id: Uuid,
    reason: &str,
) -> Result<Option<JobRecord>, sqlx::Error> {
    let job = sqlx::query_as::<_, JobRecord>(
        r"
        UPDATE jobs
        SET state = 'skipped', lease_owner = NULL, lease_expires_at = NULL,
            last_error = $2, completed_at = now(), updated_at = now()
        WHERE id = $1 AND state = 'leased'
        RETURNING *
        ",
    )
    .bind(job_id)
    .bind(reason)
    .fetch_optional(pool)
    .await?;
    if job.is_some() {
        release_resource_lease(pool, job_id).await?;
    }
    Ok(job)
}

/// Enqueues a durable scan for an enabled import source, or returns the scan
/// that is already pending or leased for that source.
///
/// Locking the source row makes concurrent requests idempotent even when one
/// request has to wait for another transaction to commit its job.
///
/// # Errors
///
/// Returns a database error when the source or queue cannot be queried.
pub async fn enqueue_import_scan(
    pool: &PgPool,
    source_id: Uuid,
) -> Result<Option<JobRecord>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let source = sqlx::query_as::<_, ImportSourceRecord>(
        "SELECT * FROM import_sources WHERE id = $1 AND enabled FOR UPDATE",
    )
    .bind(source_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(source) = source else {
        transaction.commit().await?;
        return Ok(None);
    };

    let inserted = sqlx::query_as::<_, JobRecord>(
        r"
        INSERT INTO jobs (
            id, job_type, target_node_id, import_source_id, priority, max_attempts, resource_class
        )
        VALUES ($1, 'import_scan', $2, $3, 20, 10, 'heavy_io')
        ON CONFLICT DO NOTHING
        RETURNING *
        ",
    )
    .bind(Uuid::new_v4())
    .bind(source.destination_folder_id)
    .bind(source.id)
    .fetch_optional(&mut *transaction)
    .await?;

    let job = match inserted {
        Some(job) => job,
        None => {
            sqlx::query_as::<_, JobRecord>(
                r"
                SELECT * FROM jobs
                WHERE job_type = 'import_scan'
                  AND import_source_id = $1
                  AND state IN ('pending', 'leased')
                ORDER BY created_at, id
                LIMIT 1
                ",
            )
            .bind(source.id)
            .fetch_one(&mut *transaction)
            .await?
        }
    };
    transaction.commit().await?;
    Ok(Some(job))
}

/// Atomically leases the highest-priority pending job of the requested type.
///
/// # Errors
///
/// Returns a database error when the queue cannot be queried or updated.
pub async fn claim_job(
    pool: &PgPool,
    job_type: JobType,
    owner: &str,
    lease_ttl: chrono::Duration,
) -> Result<Option<JobRecord>, sqlx::Error> {
    let lease_expires_at = Utc::now() + lease_ttl;
    sqlx::query_as::<_, JobRecord>(
        r"
        WITH candidate AS (
            SELECT id
            FROM jobs
            WHERE job_type = $1 AND state = 'pending'
              AND (
                origin <> 'backfill'
                OR EXISTS (
                    SELECT 1
                    FROM backfill_campaigns c
                    WHERE c.id = jobs.campaign_id
                      AND c.state = 'running'
                      AND (SELECT count(*) FROM jobs active
                           WHERE active.campaign_id = c.id AND active.state = 'leased') < c.max_running
                )
              )
            ORDER BY CASE origin
                         WHEN 'foreground' THEN 0
                         WHEN 'repair' THEN 1
                         WHEN 'backfill' THEN 2
                     END,
                     priority DESC, created_at, id
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        UPDATE jobs
        SET state = 'leased', lease_owner = $2, lease_expires_at = $3,
            attempts = attempts + 1, updated_at = now()
        FROM candidate
        WHERE jobs.id = candidate.id
        RETURNING jobs.*
        ",
    )
    .bind(job_type)
    .bind(owner)
    .bind(lease_expires_at)
    .fetch_optional(pool)
    .await
}

/// Claims a job and, for constrained resource classes, a shared capacity slot
/// in the same transaction. No job is leased when its resource pool is full.
///
/// # Errors
///
/// Returns a database error when the queue, fairness state, or resource slot
/// cannot be queried or updated atomically.
#[allow(clippy::too_many_lines)]
pub async fn claim_job_with_resource_lease(
    pool: &PgPool,
    job_type: JobType,
    owner: &str,
    lease_ttl: chrono::Duration,
) -> Result<Option<JobRecord>, sqlx::Error> {
    let lease_expires_at = Utc::now() + lease_ttl;
    let mut transaction = pool.begin().await?;
    sqlx::query(
        r"
        INSERT INTO job_claim_fairness (job_type)
        VALUES ($1)
        ON CONFLICT (job_type) DO NOTHING
        ",
    )
    .bind(job_type)
    .execute(&mut *transaction)
    .await?;
    let foreground_claims = sqlx::query_scalar::<_, i32>(
        r"
        SELECT foreground_claims_since_backfill
        FROM job_claim_fairness
        WHERE job_type = $1
        FOR UPDATE
        ",
    )
    .bind(job_type)
    .fetch_one(&mut *transaction)
    .await?;
    // Keep each lookup aligned with jobs_claim_origin_idx. The previous
    // all-in-one CASE expression forced PostgreSQL to sort every pending job,
    // which made claiming increasingly expensive as the queue grew.
    let preferred_backfill = sqlx::query_as::<_, JobRecord>(
        r"
        SELECT jobs.*
        FROM jobs
        JOIN backfill_campaigns c ON c.id = jobs.campaign_id
        WHERE jobs.job_type = $1 AND jobs.state = 'pending'
          AND jobs.origin = 'backfill'
          AND c.state = 'running'
          AND $2 >= c.foreground_fairness
          AND (SELECT count(*) FROM jobs active
               WHERE active.campaign_id = c.id AND active.state = 'leased') < c.max_running
        ORDER BY jobs.priority DESC, jobs.created_at, jobs.id
        FOR UPDATE OF jobs SKIP LOCKED
        LIMIT 1
        ",
    )
    .bind(job_type)
    .bind(foreground_claims)
    .fetch_optional(&mut *transaction)
    .await?;
    let foreground = if preferred_backfill.is_none() {
        sqlx::query_as::<_, JobRecord>(
            r"
            SELECT jobs.*
            FROM jobs
            WHERE job_type = $1 AND state = 'pending' AND origin <> 'backfill'
            ORDER BY origin, priority DESC, created_at, id
            FOR UPDATE SKIP LOCKED
            LIMIT 1
            ",
        )
        .bind(job_type)
        .fetch_optional(&mut *transaction)
        .await?
    } else {
        None
    };
    let fallback_backfill = if preferred_backfill.is_none() && foreground.is_none() {
        sqlx::query_as::<_, JobRecord>(
            r"
            SELECT jobs.*
            FROM jobs
            JOIN backfill_campaigns c ON c.id = jobs.campaign_id
            WHERE jobs.job_type = $1 AND jobs.state = 'pending'
              AND jobs.origin = 'backfill' AND c.state = 'running'
              AND (SELECT count(*) FROM jobs active
                   WHERE active.campaign_id = c.id AND active.state = 'leased') < c.max_running
            ORDER BY jobs.priority DESC, jobs.created_at, jobs.id
            FOR UPDATE OF jobs SKIP LOCKED
            LIMIT 1
            ",
        )
        .bind(job_type)
        .fetch_optional(&mut *transaction)
        .await?
    } else {
        None
    };
    let candidate = preferred_backfill.or(foreground).or(fallback_backfill);
    let Some(candidate) = candidate else {
        transaction.commit().await?;
        return Ok(None);
    };

    if candidate.resource_class != JobResourceClass::Light {
        let slot = sqlx::query_scalar::<_, i32>(
            r"
            WITH available AS (
                SELECT slot_number
                FROM worker_resource_leases
                WHERE resource_class = $1
                  AND (lease_expires_at IS NULL OR lease_expires_at < now())
                ORDER BY slot_number
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            UPDATE worker_resource_leases lease
            SET lease_owner = $2, job_id = $3, lease_expires_at = $4, updated_at = now()
            FROM available
            WHERE lease.resource_class = $1
              AND lease.slot_number = available.slot_number
            RETURNING lease.slot_number
            ",
        )
        .bind(candidate.resource_class)
        .bind(owner)
        .bind(candidate.id)
        .bind(lease_expires_at)
        .fetch_optional(&mut *transaction)
        .await?;
        if slot.is_none() {
            transaction.rollback().await?;
            return Ok(None);
        }
    }

    let claimed = sqlx::query_as::<_, JobRecord>(
        r"
        UPDATE jobs
        SET state = 'leased', lease_owner = $2, lease_expires_at = $3,
            attempts = attempts + 1, updated_at = now()
        WHERE id = $1 AND state = 'pending'
        RETURNING *
        ",
    )
    .bind(candidate.id)
    .bind(owner)
    .bind(lease_expires_at)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some(job) = &claimed {
        sqlx::query(
            r"
            UPDATE job_claim_fairness
            SET foreground_claims_since_backfill = CASE
                    WHEN $2 = 'foreground' THEN foreground_claims_since_backfill + 1
                    WHEN $2 = 'backfill' THEN 0
                    ELSE foreground_claims_since_backfill
                END,
                updated_at = now()
            WHERE job_type = $1
            ",
        )
        .bind(job_type)
        .bind(job.origin)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(claimed)
}

/// Extends a job lease while its owner is still processing long-running work.
///
/// # Errors
///
/// Returns a database error when the lease cannot be updated.
pub async fn renew_job_lease(
    pool: &PgPool,
    job_id: Uuid,
    owner: &str,
    lease_ttl: chrono::Duration,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r"
        UPDATE jobs
        SET lease_expires_at = $3, updated_at = now()
        WHERE id = $1 AND state = 'leased' AND lease_owner = $2
        ",
    )
    .bind(job_id)
    .bind(owner)
    .bind(Utc::now() + lease_ttl)
    .execute(pool)
    .await?;
    if result.rows_affected() == 1 {
        sqlx::query(
            r"
            UPDATE worker_resource_leases
            SET lease_expires_at = $3, updated_at = now()
            WHERE job_id = $1 AND lease_owner = $2
            ",
        )
        .bind(job_id)
        .bind(owner)
        .bind(Utc::now() + lease_ttl)
        .execute(pool)
        .await?;
    }
    Ok(result.rows_affected() == 1)
}

/// Marks a leased job completed and clears its lease.
///
/// # Errors
///
/// Returns a database error when the job cannot be updated.
pub async fn complete_job(pool: &PgPool, job_id: Uuid) -> Result<Option<JobRecord>, sqlx::Error> {
    let job = sqlx::query_as::<_, JobRecord>(
        r"
        UPDATE jobs
        SET state = 'completed', lease_owner = NULL, lease_expires_at = NULL,
            completed_at = now(), updated_at = now()
        WHERE id = $1 AND state = 'leased'
        RETURNING *
        ",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await?;
    if job.is_some() {
        release_resource_lease(pool, job_id).await?;
    }
    Ok(job)
}

/// Records a failed attempt, retrying until the job reaches `max_attempts`.
///
/// # Errors
///
/// Returns a database error when the job cannot be updated.
pub async fn fail_job(
    pool: &PgPool,
    job_id: Uuid,
    error: &str,
) -> Result<Option<JobRecord>, sqlx::Error> {
    let job = sqlx::query_as::<_, JobRecord>(
        r"
        UPDATE jobs
        SET state = CASE WHEN attempts >= max_attempts THEN 'failed'::job_state
                         ELSE 'pending'::job_state END,
            lease_owner = NULL, lease_expires_at = NULL, last_error = $2,
            updated_at = now()
        WHERE id = $1 AND state = 'leased'
        RETURNING *
        ",
    )
    .bind(job_id)
    .bind(error)
    .fetch_optional(pool)
    .await?;
    if job.is_some() {
        release_resource_lease(pool, job_id).await?;
    }
    Ok(job)
}

/// Marks a leased job failed without consuming further retry attempts.
///
/// # Errors
///
/// Returns a database error when the terminal outcome cannot be recorded.
pub async fn fail_job_terminal(
    pool: &PgPool,
    job_id: Uuid,
    error: &str,
) -> Result<Option<JobRecord>, sqlx::Error> {
    let job = sqlx::query_as::<_, JobRecord>(
        r"
        UPDATE jobs
        SET state = 'failed', attempts = max_attempts,
            lease_owner = NULL, lease_expires_at = NULL, last_error = $2,
            updated_at = now()
        WHERE id = $1 AND state = 'leased'
        RETURNING *
        ",
    )
    .bind(job_id)
    .bind(error)
    .fetch_optional(pool)
    .await?;
    if job.is_some() {
        release_resource_lease(pool, job_id).await?;
    }
    Ok(job)
}

async fn release_resource_lease(pool: &PgPool, job_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r"
        UPDATE worker_resource_leases
        SET lease_owner = NULL, job_id = NULL, lease_expires_at = NULL, updated_at = now()
        WHERE job_id = $1
        ",
    )
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Returns expired leases to the pending queue.
///
/// # Errors
///
/// Returns a database error when expired leases cannot be updated.
pub async fn release_expired_leases(pool: &PgPool) -> Result<u64, sqlx::Error> {
    sqlx::query(
        r"
        UPDATE worker_resource_leases
        SET lease_owner = NULL, job_id = NULL, lease_expires_at = NULL, updated_at = now()
        WHERE lease_expires_at < now()
        ",
    )
    .execute(pool)
    .await?;
    let result = sqlx::query(
        r"
        UPDATE jobs
        SET state = 'pending', lease_owner = NULL, lease_expires_at = NULL, updated_at = now()
        WHERE state = 'leased' AND lease_expires_at < now()
        ",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Enqueues at most ten low-priority jobs whose extractor record is not at `current_version`.
///
/// Active queue uniqueness makes repeated calls safe and allows gradual reprocessing batches.
///
/// # Errors
///
/// Returns a database error when candidates cannot be selected or enqueued.
pub async fn enqueue_reprocessing(
    pool: &PgPool,
    extractor_name: &str,
    current_version: &str,
) -> Result<u64, sqlx::Error> {
    let node_ids = sqlx::query_scalar::<_, Uuid>(
        r"
        SELECT node_id
        FROM metadata_records
        WHERE extractor_name = $1 AND extractor_version <> $2
        ORDER BY updated_at, node_id
        LIMIT 10
        ",
    )
    .bind(extractor_name)
    .bind(current_version)
    .fetch_all(pool)
    .await?;
    let mut enqueued = 0;
    for node_id in node_ids {
        if enqueue_job_with_context(
            pool,
            JobType::MetadataExtraction,
            node_id,
            -100,
            JobOrigin::Repair,
            None,
            JobResourceClass::Extractor,
        )
        .await?
        .is_some()
        {
            enqueued += 1;
        }
    }
    Ok(enqueued)
}

/// OCR candidate selection for bounded manual reprocessing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OcrReprocessScope {
    Node(Uuid),
    Failed,
    VersionMismatch(String),
}

/// Enqueues a bounded set of manual OCR jobs, ignoring already-active work.
///
/// # Errors
///
/// Returns a database error when candidates or the queue cannot be accessed.
pub async fn enqueue_ocr_reprocessing(
    pool: &PgPool,
    scope: &OcrReprocessScope,
    limit: u32,
) -> Result<u64, sqlx::Error> {
    let limit = i64::from(limit.clamp(1, 100));
    let node_ids = match scope {
        OcrReprocessScope::Node(node_id) => {
            sqlx::query_scalar::<_, Uuid>(
                r"
                SELECT n.id
                FROM nodes n
                JOIN file_objects f ON f.node_id = n.id AND f.upload_state = 'finalized'
                WHERE n.id = $1 AND n.kind = 'file' AND n.lifecycle_state = 'active'
                LIMIT 1
                ",
            )
            .bind(node_id)
            .fetch_all(pool)
            .await?
        }
        OcrReprocessScope::Failed => {
            sqlx::query_scalar::<_, Uuid>(
                r"
                SELECT n.id
                FROM nodes n
                JOIN document_text d ON d.node_id = n.id
                WHERE n.lifecycle_state = 'active' AND d.status = 'failed'
                  AND NOT EXISTS (
                      SELECT 1
                      FROM jobs j
                      WHERE j.target_node_id = n.id
                        AND j.job_type = 'ocr'
                        AND j.state IN ('pending', 'leased')
                  )
                ORDER BY d.updated_at, n.id
                LIMIT $1
                ",
            )
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        OcrReprocessScope::VersionMismatch(version) => {
            sqlx::query_scalar::<_, Uuid>(
                r"
                SELECT n.id
                FROM nodes n
                JOIN file_objects f ON f.node_id = n.id AND f.upload_state = 'finalized'
                LEFT JOIN document_text d ON d.node_id = n.id
                WHERE n.lifecycle_state = 'active'
                  AND (d.node_id IS NULL OR d.engine_version <> $1)
                  AND NOT EXISTS (
                      SELECT 1
                      FROM jobs j
                      WHERE j.target_node_id = n.id
                        AND j.job_type = 'ocr'
                        AND j.state IN ('pending', 'leased')
                  )
                ORDER BY d.updated_at NULLS FIRST, n.id
                LIMIT $2
                ",
            )
            .bind(version)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
    };
    let mut enqueued = 0;
    for node_id in node_ids {
        if enqueue_job_with_context(
            pool,
            JobType::Ocr,
            node_id,
            -100,
            JobOrigin::Repair,
            None,
            JobResourceClass::HeavyCpu,
        )
        .await?
        .is_some()
        {
            enqueued += 1;
        }
    }
    Ok(enqueued)
}

/// Fetches a job for status polling.
///
/// # Errors
/// Returns a database error when the job cannot be queried.
pub async fn get_job(pool: &PgPool, id: Uuid) -> Result<Option<JobRecord>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM jobs WHERE id=$1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// The fixed watched-folder source persisted by the v1 schema.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct ImportSourceRecord {
    pub id: Uuid,
    pub watch_path: String,
    pub destination_folder_id: Uuid,
    pub enabled: bool,
    pub last_scan_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Import-source status with durable entry counts grouped by lifecycle.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct ImportSourceStatusRecord {
    pub id: Uuid,
    pub watch_path: String,
    pub destination_folder_id: Uuid,
    pub enabled: bool,
    pub last_scan_at: Option<DateTime<Utc>>,
    pub discovered_count: i64,
    pub stable_count: i64,
    pub importing_count: i64,
    pub imported_count: i64,
    pub failed_count: i64,
}

/// One source file tracked across discovery, import, and retry.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct ImportEntryRecord {
    pub id: Uuid,
    pub source_id: Uuid,
    pub source_path: String,
    pub source_size: i64,
    pub source_modified_at: DateTime<Utc>,
    pub source_checksum: Option<String>,
    pub state: ImportEntryState,
    pub resulting_node_id: Option<Uuid>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Filesystem observations used to create or refresh an import entry.
pub struct UpsertImportEntry<'a> {
    pub source_id: Uuid,
    pub source_path: &'a str,
    pub source_size: i64,
    pub source_modified_at: DateTime<Utc>,
}

/// Data required to atomically publish one staged watched-folder file.
pub struct FinalizeImport<'a> {
    pub entry_id: Uuid,
    pub destination_folder_id: Uuid,
    pub directory_names: &'a [String],
    pub display_name: &'a str,
    pub original_storage_key: Uuid,
    pub byte_size: i64,
    pub mime_type: &'a str,
    pub checksum_sha256: &'a str,
    pub source_created_at: Option<DateTime<Utc>>,
    pub source_modified_at: DateTime<Utc>,
}

/// Fetches an import source by identifier.
///
/// # Errors
///
/// Returns the database error when the source cannot be queried.
pub async fn get_import_source(
    pool: &PgPool,
    source_id: Uuid,
) -> Result<Option<ImportSourceRecord>, sqlx::Error> {
    sqlx::query_as::<_, ImportSourceRecord>("SELECT * FROM import_sources WHERE id = $1")
        .bind(source_id)
        .fetch_optional(pool)
        .await
}

/// Lists watched-folder sources and their entry counts.
///
/// # Errors
///
/// Returns the database error when source status cannot be queried.
pub async fn list_import_source_statuses(
    pool: &PgPool,
) -> Result<Vec<ImportSourceStatusRecord>, sqlx::Error> {
    sqlx::query_as::<_, ImportSourceStatusRecord>(
        r"
        SELECT
            source.id,
            source.watch_path,
            source.destination_folder_id,
            source.enabled,
            source.last_scan_at,
            count(entry.id) FILTER (WHERE entry.state = 'discovered') AS discovered_count,
            count(entry.id) FILTER (WHERE entry.state = 'stable') AS stable_count,
            count(entry.id) FILTER (WHERE entry.state = 'importing') AS importing_count,
            count(entry.id) FILTER (WHERE entry.state = 'imported') AS imported_count,
            count(entry.id) FILTER (WHERE entry.state = 'failed') AS failed_count
        FROM import_sources AS source
        LEFT JOIN import_entries AS entry ON entry.source_id = source.id
        GROUP BY source.id
        ORDER BY source.created_at, source.id
        ",
    )
    .fetch_all(pool)
    .await
}

/// Enables or disables the fixed watched-folder source.
///
/// # Errors
///
/// Returns the database error when the source cannot be updated.
pub async fn set_import_source_enabled(
    pool: &PgPool,
    source_id: Uuid,
    enabled: bool,
) -> Result<Option<ImportSourceRecord>, sqlx::Error> {
    sqlx::query_as::<_, ImportSourceRecord>(
        r"
        UPDATE import_sources SET enabled = $2, updated_at = now()
        WHERE id = $1 RETURNING *
        ",
    )
    .bind(source_id)
    .bind(enabled)
    .fetch_optional(pool)
    .await
}

/// Records completion of a manually requested source scan.
///
/// # Errors
///
/// Returns the database error when the timestamp cannot be updated.
pub async fn mark_import_source_scanned(pool: &PgPool, source_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE import_sources SET last_scan_at = now(), updated_at = now() WHERE id = $1")
        .bind(source_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Lists entries for one source, optionally filtered to one state.
///
/// # Errors
///
/// Returns the database error when entries cannot be queried.
pub async fn list_import_entries(
    pool: &PgPool,
    source_id: Uuid,
    state: Option<ImportEntryState>,
) -> Result<Vec<ImportEntryRecord>, sqlx::Error> {
    sqlx::query_as::<_, ImportEntryRecord>(
        r"
        SELECT * FROM import_entries
        WHERE source_id = $1 AND ($2::import_entry_state IS NULL OR state = $2)
        ORDER BY updated_at DESC, id
        ",
    )
    .bind(source_id)
    .bind(state)
    .fetch_all(pool)
    .await
}

/// Lists import entries changed after a stable event-stream cursor.
///
/// The cursor uses both the update timestamp and identifier so entries that
/// share a database timestamp are delivered exactly once.
///
/// # Errors
///
/// Returns the database error when entries cannot be queried.
pub async fn list_import_entry_events_after(
    pool: &PgPool,
    source_id: Uuid,
    updated_after: DateTime<Utc>,
    id_after: Uuid,
    limit: i64,
) -> Result<Vec<ImportEntryRecord>, sqlx::Error> {
    sqlx::query_as::<_, ImportEntryRecord>(
        r"
        SELECT * FROM import_entries
        WHERE source_id = $1 AND (updated_at, id) > ($2, $3)
        ORDER BY updated_at, id
        LIMIT $4
        ",
    )
    .bind(source_id)
    .bind(updated_after)
    .bind(id_after)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Resets one failed source entry for a user-requested retry.
///
/// # Errors
///
/// Returns the database error when the entry cannot be updated.
pub async fn retry_import_entry(
    pool: &PgPool,
    source_id: Uuid,
    entry_id: Uuid,
) -> Result<Option<ImportEntryRecord>, sqlx::Error> {
    sqlx::query_as::<_, ImportEntryRecord>(
        r"
        UPDATE import_entries
        SET state = 'discovered', error_message = NULL, updated_at = now()
        WHERE id = $1 AND source_id = $2 AND state = 'failed'
        RETURNING *
        ",
    )
    .bind(entry_id)
    .bind(source_id)
    .fetch_optional(pool)
    .await
}

/// Creates or refreshes a discovered entry. A path that reappears after its
/// previous source was moved is treated as a new attempt on the same row.
///
/// # Errors
///
/// Returns the database error when the entry cannot be persisted.
pub async fn upsert_import_entry(
    pool: &PgPool,
    input: UpsertImportEntry<'_>,
) -> Result<ImportEntryRecord, sqlx::Error> {
    sqlx::query_as::<_, ImportEntryRecord>(
        r"
        INSERT INTO import_entries (
            id, source_id, source_path, source_size, source_modified_at
        )
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (source_id, source_path) DO UPDATE
        SET
            source_size = EXCLUDED.source_size,
            source_modified_at = EXCLUDED.source_modified_at,
            state = CASE
                WHEN import_entries.state = 'imported' THEN 'discovered'
                ELSE import_entries.state
            END,
            resulting_node_id = CASE
                WHEN import_entries.state = 'imported' THEN NULL
                ELSE import_entries.resulting_node_id
            END,
            source_checksum = CASE
                WHEN import_entries.state = 'imported' THEN NULL
                ELSE import_entries.source_checksum
            END,
            updated_at = now()
        RETURNING *
        ",
    )
    .bind(Uuid::new_v4())
    .bind(input.source_id)
    .bind(input.source_path)
    .bind(input.source_size)
    .bind(input.source_modified_at)
    .fetch_one(pool)
    .await
}

/// Lists work eligible for this source's next manual import pass.
///
/// # Errors
///
/// Returns the database error when pending entries cannot be queried.
pub async fn list_pending_entries(
    pool: &PgPool,
    source_id: Uuid,
) -> Result<Vec<ImportEntryRecord>, sqlx::Error> {
    sqlx::query_as::<_, ImportEntryRecord>(
        r"
        SELECT * FROM import_entries
        WHERE source_id = $1 AND state IN ('discovered', 'stable', 'importing')
        ORDER BY created_at, id
        ",
    )
    .bind(source_id)
    .fetch_all(pool)
    .await
}

/// Moves an import entry to stable after its source survives staging unchanged.
///
/// # Errors
///
/// Returns the database error when the entry cannot be updated.
pub async fn mark_import_stable(
    pool: &PgPool,
    entry_id: Uuid,
) -> Result<ImportEntryRecord, sqlx::Error> {
    set_import_entry_state(pool, entry_id, ImportEntryState::Stable).await
}

/// Checkpoints an entry immediately before its staged object is finalized.
///
/// # Errors
///
/// Returns the database error when the stable entry cannot be updated.
pub async fn mark_importing(
    pool: &PgPool,
    entry_id: Uuid,
) -> Result<ImportEntryRecord, sqlx::Error> {
    sqlx::query_as::<_, ImportEntryRecord>(
        r"
        UPDATE import_entries
        SET state = 'importing', error_message = NULL, updated_at = now()
        WHERE id = $1 AND state IN ('stable', 'importing')
        RETURNING *
        ",
    )
    .bind(entry_id)
    .fetch_one(pool)
    .await
}

/// Lists entries left at the durable importing checkpoint by an interruption.
///
/// # Errors
///
/// Returns the database error when interrupted entries cannot be queried.
pub async fn list_importing_entries(pool: &PgPool) -> Result<Vec<ImportEntryRecord>, sqlx::Error> {
    sqlx::query_as::<_, ImportEntryRecord>(
        "SELECT * FROM import_entries WHERE state = 'importing' ORDER BY updated_at, id",
    )
    .fetch_all(pool)
    .await
}

/// Returns an entry to discovery after its source changes during staging.
///
/// # Errors
///
/// Returns the database error when the entry cannot be updated.
pub async fn reset_import_discovered(
    pool: &PgPool,
    entry_id: Uuid,
) -> Result<ImportEntryRecord, sqlx::Error> {
    set_import_entry_state(pool, entry_id, ImportEntryState::Discovered).await
}

async fn set_import_entry_state(
    pool: &PgPool,
    entry_id: Uuid,
    state: ImportEntryState,
) -> Result<ImportEntryRecord, sqlx::Error> {
    sqlx::query_as::<_, ImportEntryRecord>(
        r"
        UPDATE import_entries
        SET state = $2, error_message = NULL, updated_at = now()
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(entry_id)
    .bind(state)
    .fetch_one(pool)
    .await
}

/// Records a successfully finalized import and its integrity checksum.
///
/// # Errors
///
/// Returns the database error when the entry cannot be updated.
pub async fn mark_imported(
    pool: &PgPool,
    entry_id: Uuid,
    node_id: Uuid,
    checksum: &str,
) -> Result<ImportEntryRecord, sqlx::Error> {
    sqlx::query_as::<_, ImportEntryRecord>(
        r"
        UPDATE import_entries
        SET state = 'imported', resulting_node_id = $2, source_checksum = $3,
            error_message = NULL, updated_at = now()
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(entry_id)
    .bind(node_id)
    .bind(checksum)
    .fetch_one(pool)
    .await
}

/// Persists an actionable import failure for later inspection and retry.
///
/// # Errors
///
/// Returns the database error when the entry cannot be updated.
pub async fn mark_import_failed(
    pool: &PgPool,
    entry_id: Uuid,
    message: &str,
) -> Result<ImportEntryRecord, sqlx::Error> {
    sqlx::query_as::<_, ImportEntryRecord>(
        r"
        UPDATE import_entries
        SET state = 'failed', error_message = $2, updated_at = now()
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(entry_id)
    .bind(message)
    .fetch_one(pool)
    .await
}

/// Atomically recreates source folders, publishes a file/object, enqueues
/// metadata extraction, and completes its import entry.
///
/// Matching active folders are reused. Any matching file or final file-name
/// collision fails the entire transaction.
///
/// # Errors
///
/// Returns `NotFound`, `NotReady`, or `NameConflict` for expected import
/// failures, and `Database` for other persistence errors.
pub async fn finalize_import(
    pool: &PgPool,
    input: FinalizeImport<'_>,
) -> Result<NodeRecord, FinalizeImportError> {
    let mut transaction = pool.begin().await?;
    let entry = sqlx::query_as::<_, ImportEntryRecord>(
        "SELECT * FROM import_entries WHERE id = $1 FOR UPDATE",
    )
    .bind(input.entry_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(FinalizeImportError::NotFound)?;
    if entry.state == ImportEntryState::Imported {
        let node_id = entry.resulting_node_id.ok_or_else(|| {
            sqlx::Error::Protocol("imported entry has no resulting node".to_owned())
        })?;
        let node = load_node_in_transaction(&mut transaction, node_id)
            .await?
            .ok_or(FinalizeImportError::NotFound)?;
        transaction.commit().await?;
        return Ok(node);
    }
    if entry.state != ImportEntryState::Stable && entry.state != ImportEntryState::Importing {
        return Err(FinalizeImportError::NotReady);
    }
    ensure_import_destination(&mut transaction, input.destination_folder_id).await?;

    let mut parent_id = input.destination_folder_id;
    for name in input.directory_names {
        parent_id = find_or_create_import_folder(&mut transaction, parent_id, name).await?;
    }
    let node = sqlx::query_as::<_, NodeRecord>(
        r"
        INSERT INTO nodes (
            id, parent_id, name, kind, source_created_at, source_modified_at
        )
        VALUES ($1, $2, $3, 'file', $4, $5)
        RETURNING
            id, parent_id, name, kind, lifecycle_state, source_created_at,
            source_modified_at, created_at, updated_at
        ",
    )
    .bind(Uuid::new_v4())
    .bind(parent_id)
    .bind(input.display_name)
    .bind(input.source_created_at)
    .bind(input.source_modified_at)
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_import_finalize_error)?;
    insert_finalized_object(
        &mut transaction,
        node.id,
        input.original_storage_key,
        input.byte_size,
        input.mime_type,
        input.checksum_sha256,
    )
    .await?;
    enqueue_metadata_job(&mut transaction, node.id).await?;
    enqueue_ocr_job_best_effort(&mut transaction, node.id).await;
    enqueue_email_job_best_effort(&mut transaction, node.id).await;
    sqlx::query(
        r"
        UPDATE import_entries
        SET state = 'imported', resulting_node_id = $2, source_checksum = $3,
            error_message = NULL, updated_at = now()
        WHERE id = $1
        ",
    )
    .bind(input.entry_id)
    .bind(node.id)
    .bind(input.checksum_sha256)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(node)
}

async fn ensure_import_destination(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    folder_id: Uuid,
) -> Result<(), FinalizeImportError> {
    let exists: bool = sqlx::query_scalar(
        r"
        SELECT EXISTS (
            SELECT 1 FROM nodes
            WHERE id = $1 AND kind = 'folder' AND lifecycle_state = 'active'
        )
        ",
    )
    .bind(folder_id)
    .fetch_one(&mut **transaction)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(FinalizeImportError::NotFound)
    }
}

async fn find_or_create_import_folder(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    parent_id: Uuid,
    name: &str,
) -> Result<Uuid, FinalizeImportError> {
    let existing: Option<(Uuid, NodeKind)> = sqlx::query_as(
        r"
        SELECT id, kind FROM nodes
        WHERE parent_id = $1 AND name = $2 AND lifecycle_state = 'active'
        ",
    )
    .bind(parent_id)
    .bind(name)
    .fetch_optional(&mut **transaction)
    .await?;
    match existing {
        Some((id, NodeKind::Folder)) => Ok(id),
        Some((_, NodeKind::File)) => Err(FinalizeImportError::NameConflict),
        None => sqlx::query_scalar(
            "INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'folder') RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(parent_id)
        .bind(name)
        .fetch_one(&mut **transaction)
        .await
        .map_err(map_import_finalize_error),
    }
}

fn map_import_finalize_error(error: sqlx::Error) -> FinalizeImportError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        FinalizeImportError::NameConflict
    } else {
        FinalizeImportError::Database(error)
    }
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

/// A favorited node with its details, sorted by favorited time.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct FavoriteRecord {
    pub node_id: Uuid,
    pub favorited_at: DateTime<Utc>,
    pub name: String,
    pub kind: NodeKind,
    pub parent_id: Option<Uuid>,
    pub lifecycle_state: LifecycleState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Typed representation of a row in `trash_entries` joined with node details.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct TrashEntryRecord {
    pub id: Uuid,
    pub node_id: Uuid,
    pub original_parent_id: Option<Uuid>,
    pub trashed_at: DateTime<Utc>,
    pub scheduled_purge_at: DateTime<Utc>,
    pub name: String,
    pub kind: NodeKind,
    pub parent_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Expected failures for trash and restore mutations.
#[derive(Debug, thiserror::Error)]
pub enum TrashMutationError {
    #[error(transparent)]
    Rule(#[from] FolderError),
    #[error("the root folder cannot be trashed")]
    CannotTrashRoot,
    #[error("the node is not in the trash")]
    NotTrashed,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
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

/// Active finalized file fields needed to serve a download.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct DownloadFileRecord {
    pub node_id: Uuid,
    pub display_name: String,
    pub storage_key: String,
    pub byte_size: i64,
    pub mime_type: Option<String>,
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

/// Expected failures while atomically publishing a watched-folder import.
#[derive(Debug, thiserror::Error)]
pub enum FinalizeImportError {
    #[error("import entry or destination folder was not found")]
    NotFound,
    #[error("import entry is not ready for finalization")]
    NotReady,
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

/// Loads an active file and its finalized object for streaming.
///
/// # Errors
///
/// Returns the database error when the query cannot be completed.
pub async fn get_download_file(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Option<DownloadFileRecord>, sqlx::Error> {
    sqlx::query_as::<_, DownloadFileRecord>(
        r"
        SELECT
            nodes.id AS node_id,
            nodes.name AS display_name,
            file_objects.storage_key,
            file_objects.byte_size,
            file_objects.mime_type
        FROM nodes
        JOIN file_objects ON file_objects.node_id = nodes.id
        WHERE nodes.id = $1
          AND nodes.kind = 'file'
          AND nodes.lifecycle_state = 'active'
          AND file_objects.upload_state = 'finalized'
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
    enqueue_ocr_job_best_effort(&mut transaction, node.id).await;
    enqueue_email_job_best_effort(&mut transaction, node.id).await;
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
        INSERT INTO jobs (id, job_type, target_node_id, resource_class)
        VALUES ($1, 'metadata_extraction', $2, 'extractor')
        ON CONFLICT DO NOTHING
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// Enqueues a foreground email-extraction job alongside finalization.
///
/// Enqueueing is unconditional for every finalized file rather than gated on
/// the declared MIME: the type is only reliably known after byte inspection,
/// which the handler performs. A non-email file is recorded `unsupported` there
/// and costs one cheap sniff. The savepoint means a failed enqueue can never
/// roll back the finalization itself — the file stays published, and the
/// missing job is recoverable through the reprocess scopes.
async fn enqueue_email_job_best_effort(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
) {
    if sqlx::query("SAVEPOINT enqueue_email_job")
        .execute(&mut **transaction)
        .await
        .is_err()
    {
        return;
    }
    let inserted = sqlx::query(
        r"
        INSERT INTO jobs (
            id, job_type, target_node_id, priority, max_attempts, resource_class
        )
        VALUES ($1, 'email_extraction', $2, -5, 3, 'heavy_cpu')
        ON CONFLICT DO NOTHING
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .execute(&mut **transaction)
    .await;
    if inserted.is_err() {
        let _ = sqlx::query("ROLLBACK TO SAVEPOINT enqueue_email_job")
            .execute(&mut **transaction)
            .await;
    }
    let _ = sqlx::query("RELEASE SAVEPOINT enqueue_email_job")
        .execute(&mut **transaction)
        .await;
}

async fn enqueue_ocr_job_best_effort(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
) {
    if sqlx::query("SAVEPOINT enqueue_ocr_job")
        .execute(&mut **transaction)
        .await
        .is_err()
    {
        return;
    }
    let inserted = sqlx::query(
        r"
        INSERT INTO jobs (
            id, job_type, target_node_id, priority, max_attempts, resource_class
        )
        VALUES ($1, 'ocr', $2, -10, 5, 'heavy_cpu')
        ON CONFLICT DO NOTHING
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .execute(&mut **transaction)
    .await;
    if inserted.is_err() {
        let _ = sqlx::query("ROLLBACK TO SAVEPOINT enqueue_ocr_job")
            .execute(&mut **transaction)
            .await;
    }
    let _ = sqlx::query("RELEASE SAVEPOINT enqueue_ocr_job")
        .execute(&mut **transaction)
        .await;
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

/// Column used to sort folder children (folders always sort before files).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChildrenSort {
    #[default]
    Name,
    Kind,
    Size,
    UpdatedAt,
    CreatedAt,
}

/// Sort direction for folder children.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

/// Kind filter applied to folder children listings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildrenKindFilter {
    Folder,
    Image,
    Document,
    Video,
    Audio,
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
    list_children_page_sorted(
        pool,
        parent_id,
        cursor,
        limit,
        ChildrenSort::Name,
        SortOrder::Asc,
        &[],
    )
    .await
}

/// Lists active children with sort column, direction, and optional kind filters.
///
/// Folders always appear before files. Name-ascending cursor pagination is
/// preserved for the default sort; other modes return a single limited page.
///
/// # Errors
///
/// Returns the database error when the query cannot be completed.
pub async fn list_children_page_sorted(
    pool: &PgPool,
    parent_id: Uuid,
    cursor: Option<Uuid>,
    limit: u32,
    sort: ChildrenSort,
    order: SortOrder,
    kinds: &[ChildrenKindFilter],
) -> Result<Vec<NodeRecord>, sqlx::Error> {
    let order_sql = match order {
        SortOrder::Asc => "ASC",
        SortOrder::Desc => "DESC",
    };
    let sort_sql = match sort {
        ChildrenSort::Name => "n.name",
        ChildrenSort::Kind => "n.kind::text",
        ChildrenSort::Size => "COALESCE(fo.byte_size, 0)",
        ChildrenSort::UpdatedAt => "n.updated_at",
        ChildrenSort::CreatedAt => "n.created_at",
    };

    let mut kind_clauses = Vec::new();
    for kind in kinds {
        kind_clauses.push(match kind {
            ChildrenKindFilter::Folder => "n.kind = 'folder'".to_owned(),
            ChildrenKindFilter::Image => "n.kind = 'file' AND nm.media_kind = 'image'".to_owned(),
            ChildrenKindFilter::Document => {
                "n.kind = 'file' AND nm.media_kind = 'document'".to_owned()
            }
            ChildrenKindFilter::Video => "n.kind = 'file' AND nm.media_kind = 'video'".to_owned(),
            ChildrenKindFilter::Audio => "n.kind = 'file' AND nm.media_kind = 'audio'".to_owned(),
        });
    }
    let kind_filter = if kind_clauses.is_empty() {
        "TRUE".to_owned()
    } else {
        format!("({})", kind_clauses.join(" OR "))
    };

    let use_name_cursor =
        matches!(sort, ChildrenSort::Name) && matches!(order, SortOrder::Asc) && kinds.is_empty();

    let sql = format!(
        r"
        SELECT
            n.id,
            n.parent_id,
            n.name,
            n.kind,
            n.lifecycle_state,
            n.source_created_at,
            n.source_modified_at,
            n.created_at,
            n.updated_at
        FROM nodes AS n
        LEFT JOIN file_objects AS fo
            ON fo.node_id = n.id AND fo.upload_state = 'finalized'
        LEFT JOIN node_metadata AS nm ON nm.node_id = n.id
        WHERE n.parent_id = $1
          AND n.lifecycle_state = 'active'
          AND ({kind_filter})
          AND (
              NOT $4::bool
              OR $2::uuid IS NULL
              OR (n.name, n.id) > (
                  SELECT name, id
                  FROM nodes
                  WHERE id = $2 AND parent_id = $1
              )
          )
        ORDER BY
            CASE WHEN n.kind = 'folder' THEN 0 ELSE 1 END,
            {sort_sql} {order_sql},
            n.id
        LIMIT $3
        "
    );

    // Sort/filter fragments come only from enums above, never user input.
    sqlx::query_as::<_, NodeRecord>(sqlx::AssertSqlSafe(sql))
        .bind(parent_id)
        .bind(cursor)
        .bind(i64::from(limit))
        .bind(use_name_cursor)
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

/// Moves one active node and all of its active descendants into trash.
///
/// Only the explicitly trashed root receives a `trash_entries` row. Descendants
/// become `trashed` in the same transaction so normal listings exclude them.
///
/// # Errors
///
/// Returns `CannotTrashRoot` for the stable root, `NotFound` for missing or
/// already-trashed nodes, or a database error.
pub async fn trash_node(pool: &PgPool, node_id: Uuid) -> Result<NodeRecord, TrashMutationError> {
    trash_nodes(pool, &[node_id])
        .await?
        .into_iter()
        .next()
        .ok_or(FolderError::NotFound.into())
}

/// Moves one or more active nodes (and their active descendants) into trash.
///
/// All selected roots and every descendant transition in a single transaction.
/// Nested selections under another selected root are de-duplicated.
///
/// # Errors
///
/// Returns `CannotTrashRoot` if the root is selected, `NotFound` if any id is
/// missing or not active, or a database error.
pub async fn trash_nodes(
    pool: &PgPool,
    node_ids: &[Uuid],
) -> Result<Vec<NodeRecord>, TrashMutationError> {
    let mut ids = node_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    if ids.contains(&ROOT_NODE_ID) {
        return Err(TrashMutationError::CannotTrashRoot);
    }

    let mut transaction = pool.begin().await?;
    let roots = sqlx::query_as::<_, NodeRecord>(
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
          AND lifecycle_state = 'active'
        FOR UPDATE
        ",
    )
    .bind(&ids)
    .fetch_all(&mut *transaction)
    .await?;

    if roots.len() != ids.len() {
        return Err(FolderError::NotFound.into());
    }

    // Prefer outer roots when a selection includes both a folder and its child.
    let ancestry = ancestor_map_for_nodes(&mut transaction, &ids).await?;
    let mut top_level: Vec<NodeRecord> = Vec::new();
    for root in roots {
        let nested = ancestry
            .get(&root.id)
            .is_some_and(|ancestors| ancestors.iter().any(|ancestor| ids.contains(ancestor)));
        if !nested {
            top_level.push(root);
        }
    }

    let top_level_ids: Vec<Uuid> = top_level.iter().map(|node| node.id).collect();
    let all_ids = collect_active_subtree_ids(&mut transaction, &top_level_ids).await?;

    sqlx::query(
        r"
        UPDATE nodes
        SET lifecycle_state = 'trashed', updated_at = now()
        WHERE id = ANY($1)
          AND lifecycle_state = 'active'
        ",
    )
    .bind(&all_ids)
    .execute(&mut *transaction)
    .await?;

    for root in &top_level {
        sqlx::query(
            r"
            INSERT INTO trash_entries (id, node_id, original_parent_id)
            VALUES ($1, $2, $3)
            ",
        )
        .bind(Uuid::new_v4())
        .bind(root.id)
        .bind(root.parent_id)
        .execute(&mut *transaction)
        .await?;
    }

    // Favorites do not survive trash (Story 6.5).
    sqlx::query("DELETE FROM favorites WHERE node_id = ANY($1)")
        .bind(&all_ids)
        .execute(&mut *transaction)
        .await?;

    let trashed = sqlx::query_as::<_, NodeRecord>(
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
        ORDER BY name, id
        ",
    )
    .bind(&top_level_ids)
    .fetch_all(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(trashed)
}

/// Restores a top-level trashed node and its trashed descendants to active.
///
/// When the original parent is missing or no longer active, the node is
/// restored under the stable root.
///
/// # Errors
///
/// Returns `NotTrashed` when no trash entry exists, `NameConflict` when an
/// active sibling blocks the restore destination, `NotFound` for missing nodes,
/// or a database error.
pub async fn restore_node(pool: &PgPool, node_id: Uuid) -> Result<NodeRecord, TrashMutationError> {
    let mut transaction = pool.begin().await?;
    let (entry_id, original_parent_id) = lock_trash_entry(&mut transaction, node_id).await?;
    let node = lock_trashed_node(&mut transaction, node_id).await?;
    let restore_parent =
        resolve_restore_parent(&mut transaction, original_parent_id.or(node.parent_id)).await?;
    ensure_restore_name_available(&mut transaction, restore_parent, &node.name, node_id).await?;

    let subtree_ids = collect_trashed_subtree_ids(&mut transaction, node_id).await?;
    reactivate_trashed_subtree(&mut transaction, node_id, restore_parent, &subtree_ids).await?;
    sqlx::query("DELETE FROM trash_entries WHERE id = $1")
        .bind(entry_id)
        .execute(&mut *transaction)
        .await?;

    let restored = get_node_by_id_in_tx(&mut transaction, node_id)
        .await?
        .ok_or(FolderError::NotFound)?;
    transaction.commit().await?;
    Ok(restored)
}

/// Storage keys attached to a node that must be removed during permanent deletion.
#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct NodeStorageKeys {
    pub node_id: Uuid,
    pub original_storage_key: Option<String>,
    pub artifact_storage_keys: Vec<String>,
}

/// Enqueues permanent deletion for a trashed node. Idempotent when the node is
/// already gone or a pending/leased job already exists.
///
/// # Errors
///
/// Returns `NotTrashed` when the node is active, or a database error.
pub async fn request_permanent_deletion(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Option<JobRecord>, TrashMutationError> {
    let node = get_node_by_id(pool, node_id).await?;
    let Some(node) = node else {
        // Already deleted — treat as success with no new job.
        return Ok(None);
    };
    if node.lifecycle_state != LifecycleState::Trashed {
        return Err(TrashMutationError::NotTrashed);
    }
    if node.id == ROOT_NODE_ID {
        return Err(TrashMutationError::CannotTrashRoot);
    }

    let job = enqueue_job(pool, JobType::PermanentDeletion, node_id, 10).await?;
    Ok(job)
}

/// Collects original and artifact storage keys for a trashed node and its
/// trashed descendants.
///
/// # Errors
///
/// Returns a database error when the query cannot be completed.
pub async fn list_storage_keys_for_deletion(
    pool: &PgPool,
    root_node_id: Uuid,
) -> Result<Vec<NodeStorageKeys>, sqlx::Error> {
    sqlx::query_as::<_, NodeStorageKeys>(
        r"
        WITH RECURSIVE tree AS (
            SELECT id
            FROM nodes
            WHERE id = $1
              AND lifecycle_state = 'trashed'

            UNION ALL

            SELECT child.id
            FROM nodes AS child
            JOIN tree AS parent ON child.parent_id = parent.id
            WHERE child.lifecycle_state = 'trashed'
        )
        SELECT
            t.id AS node_id,
            (
                SELECT fo.storage_key
                FROM file_objects AS fo
                WHERE fo.node_id = t.id
                  AND fo.upload_state = 'finalized'
                LIMIT 1
            ) AS original_storage_key,
            COALESCE(
                (
                    -- Both sources live in the artifacts namespace and are
                    -- reclaimed identically, so they are collected together.
                    -- Omitting attachment artifacts would leave their bytes on
                    -- disk after the message they belong to was deleted.
                    SELECT array_agg(storage_key)
                    FROM (
                        SELECT da.storage_key
                        FROM derived_artifacts AS da
                        WHERE da.node_id = t.id
                        UNION ALL
                        SELECT eaa.storage_key
                        FROM email_attachment_artifacts AS eaa
                        WHERE eaa.node_id = t.id AND eaa.storage_key IS NOT NULL
                    ) AS keys
                ),
                '{}'::text[]
            ) AS artifact_storage_keys
        FROM tree AS t
        ",
    )
    .bind(root_node_id)
    .fetch_all(pool)
    .await
}

/// Deletes database rows for a trashed node subtree after storage objects are gone.
///
/// Removes `file_objects` first (restrict FK), then the `nodes` rows. Cascades
/// clear metadata, streams, artifacts, trash entries, and jobs.
///
/// # Errors
///
/// Returns a database error when the purge cannot be completed.
pub async fn purge_trashed_node_records(
    pool: &PgPool,
    root_node_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let node_ids = sqlx::query_scalar::<_, Uuid>(
        r"
        WITH RECURSIVE tree AS (
            SELECT id
            FROM nodes
            WHERE id = $1
              AND lifecycle_state = 'trashed'

            UNION ALL

            SELECT child.id
            FROM nodes AS child
            JOIN tree AS parent ON child.parent_id = parent.id
            WHERE child.lifecycle_state = 'trashed'
        )
        SELECT id FROM tree
        ",
    )
    .bind(root_node_id)
    .fetch_all(&mut *transaction)
    .await?;

    if node_ids.is_empty() {
        transaction.commit().await?;
        return Ok(0);
    }

    sqlx::query("DELETE FROM file_objects WHERE node_id = ANY($1)")
        .bind(&node_ids)
        .execute(&mut *transaction)
        .await?;

    // Upload sessions keep a restrict FK on the completed node.
    sqlx::query(
        "UPDATE upload_sessions SET completed_node_id = NULL WHERE completed_node_id = ANY($1)",
    )
    .bind(&node_ids)
    .execute(&mut *transaction)
    .await?;

    // Break parent links among the purge set so deletes are not ordered by depth.
    sqlx::query(
        r"
        UPDATE nodes
        SET parent_id = NULL
        WHERE id = ANY($1)
          AND parent_id = ANY($1)
        ",
    )
    .bind(&node_ids)
    .execute(&mut *transaction)
    .await?;

    let result = sqlx::query("DELETE FROM nodes WHERE id = ANY($1)")
        .bind(&node_ids)
        .execute(&mut *transaction)
        .await?;

    transaction.commit().await?;
    Ok(result.rows_affected())
}

/// Lists top-level trashed items ordered by most recently trashed first.
///
/// # Errors
///
/// Returns a database error when the query cannot be completed.
pub async fn list_trash(pool: &PgPool) -> Result<Vec<TrashEntryRecord>, sqlx::Error> {
    sqlx::query_as::<_, TrashEntryRecord>(
        r"
        SELECT
            te.id,
            te.node_id,
            te.original_parent_id,
            te.trashed_at,
            te.scheduled_purge_at,
            n.name,
            n.kind,
            n.parent_id,
            n.created_at,
            n.updated_at
        FROM trash_entries AS te
        JOIN nodes AS n ON n.id = te.node_id
        WHERE n.lifecycle_state = 'trashed'
        ORDER BY te.trashed_at DESC, n.name, n.id
        ",
    )
    .fetch_all(pool)
    .await
}

/// Returns trash entries whose scheduled purge time has elapsed.
///
/// # Errors
///
/// Returns a database error when the query cannot be completed.
pub async fn list_expired_trash(
    pool: &PgPool,
    limit: u32,
) -> Result<Vec<TrashEntryRecord>, sqlx::Error> {
    sqlx::query_as::<_, TrashEntryRecord>(
        r"
        SELECT
            te.id,
            te.node_id,
            te.original_parent_id,
            te.trashed_at,
            te.scheduled_purge_at,
            n.name,
            n.kind,
            n.parent_id,
            n.created_at,
            n.updated_at
        FROM trash_entries AS te
        JOIN nodes AS n ON n.id = te.node_id
        WHERE n.lifecycle_state = 'trashed'
          AND te.scheduled_purge_at <= now()
        ORDER BY te.scheduled_purge_at, te.id
        LIMIT $1
        ",
    )
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
}

/// Enqueues permanent-deletion jobs for expired trash entries (batch limited).
///
/// Skips nodes that already have a pending or leased permanent-deletion job so
/// each batch fills with new work. Safe to run repeatedly.
///
/// # Errors
///
/// Returns a database error when listing or enqueueing fails.
pub async fn enqueue_expired_trash_deletions(
    pool: &PgPool,
    limit: u32,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r"
        WITH candidates AS (
            SELECT te.node_id
            FROM trash_entries AS te
            JOIN nodes AS n ON n.id = te.node_id
            WHERE n.lifecycle_state = 'trashed'
              AND te.scheduled_purge_at <= now()
              AND NOT EXISTS (
                  SELECT 1
                  FROM jobs AS j
                  WHERE j.target_node_id = te.node_id
                    AND j.job_type = 'permanent_deletion'
                    AND j.state IN ('pending', 'leased')
              )
            ORDER BY te.scheduled_purge_at, te.id
            LIMIT $1
        )
        INSERT INTO jobs (id, job_type, target_node_id, priority)
        SELECT gen_random_uuid(), 'permanent_deletion', node_id, 5
        FROM candidates
        ON CONFLICT DO NOTHING
        ",
    )
    .bind(i64::from(limit))
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Adds a favorite for an active node. Idempotent when already favorited.
///
/// # Errors
///
/// Returns `NotFound` when the node is missing or not active, or a database error.
pub async fn add_favorite(pool: &PgPool, node_id: Uuid) -> Result<(), FolderMutationError> {
    if node_id == ROOT_NODE_ID {
        return Err(FolderError::NotFound.into());
    }
    let result = sqlx::query(
        r"
        INSERT INTO favorites (node_id)
        SELECT id FROM nodes
        WHERE id = $1 AND lifecycle_state = 'active'
        ON CONFLICT (node_id) DO NOTHING
        ",
    )
    .bind(node_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        let exists = get_node_by_id(pool, node_id)
            .await?
            .is_some_and(|node| node.lifecycle_state == LifecycleState::Active);
        if exists {
            // Already favorited.
            return Ok(());
        }
        return Err(FolderError::NotFound.into());
    }
    Ok(())
}

/// Removes a favorite. Idempotent when the favorite is already absent.
///
/// # Errors
///
/// Returns a database error when the delete cannot be completed.
pub async fn remove_favorite(pool: &PgPool, node_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM favorites WHERE node_id = $1")
        .bind(node_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Lists active favorited nodes ordered by most recently favorited first.
///
/// # Errors
///
/// Returns a database error when the query cannot be completed.
pub async fn list_favorites(pool: &PgPool) -> Result<Vec<FavoriteRecord>, sqlx::Error> {
    sqlx::query_as::<_, FavoriteRecord>(
        r"
        SELECT
            f.node_id,
            f.created_at AS favorited_at,
            n.name,
            n.kind,
            n.parent_id,
            n.lifecycle_state,
            n.created_at,
            n.updated_at
        FROM favorites AS f
        JOIN nodes AS n ON n.id = f.node_id
        WHERE n.lifecycle_state = 'active'
        ORDER BY f.created_at DESC, n.name, n.id
        ",
    )
    .fetch_all(pool)
    .await
}

/// Returns whether each of the given node ids is favorited.
///
/// # Errors
///
/// Returns a database error when the query cannot be completed.
pub async fn favorite_ids_among(
    pool: &PgPool,
    node_ids: &[Uuid],
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>(
        r"
        SELECT node_id FROM favorites WHERE node_id = ANY($1)
        ",
    )
    .bind(node_ids)
    .fetch_all(pool)
    .await
}

async fn lock_trash_entry(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
) -> Result<(Uuid, Option<Uuid>), TrashMutationError> {
    let entry = sqlx::query_as::<_, (Uuid, Option<Uuid>)>(
        r"
        SELECT id, original_parent_id
        FROM trash_entries
        WHERE node_id = $1
        FOR UPDATE
        ",
    )
    .bind(node_id)
    .fetch_optional(&mut **transaction)
    .await?;
    entry.ok_or(TrashMutationError::NotTrashed)
}

async fn lock_trashed_node(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
) -> Result<NodeRecord, TrashMutationError> {
    sqlx::query_as::<_, NodeRecord>(
        r"
        SELECT
            id, parent_id, name, kind, lifecycle_state,
            source_created_at, source_modified_at, created_at, updated_at
        FROM nodes
        WHERE id = $1 AND lifecycle_state = 'trashed'
        FOR UPDATE
        ",
    )
    .bind(node_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| FolderError::NotFound.into())
}

async fn resolve_restore_parent(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    preferred: Option<Uuid>,
) -> Result<Uuid, sqlx::Error> {
    let Some(parent_id) = preferred else {
        return Ok(ROOT_NODE_ID);
    };
    let parent_active = sqlx::query_scalar::<_, bool>(
        r"
        SELECT EXISTS (
            SELECT 1 FROM nodes
            WHERE id = $1 AND kind = 'folder' AND lifecycle_state = 'active'
        )
        ",
    )
    .bind(parent_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(if parent_active {
        parent_id
    } else {
        ROOT_NODE_ID
    })
}

async fn ensure_restore_name_available(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    parent_id: Uuid,
    name: &str,
    node_id: Uuid,
) -> Result<(), TrashMutationError> {
    let name_conflict = sqlx::query_scalar::<_, bool>(
        r"
        SELECT EXISTS (
            SELECT 1 FROM nodes
            WHERE parent_id = $1 AND name = $2
              AND lifecycle_state = 'active' AND id <> $3
        )
        ",
    )
    .bind(parent_id)
    .bind(name)
    .bind(node_id)
    .fetch_one(&mut **transaction)
    .await?;
    if name_conflict {
        Err(FolderError::NameConflict.into())
    } else {
        Ok(())
    }
}

async fn reactivate_trashed_subtree(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
    restore_parent: Uuid,
    subtree_ids: &[Uuid],
) -> Result<(), TrashMutationError> {
    sqlx::query(
        r"
        UPDATE nodes
        SET
            parent_id = CASE WHEN id = $1 THEN $2 ELSE parent_id END,
            lifecycle_state = 'active',
            updated_at = now()
        WHERE id = ANY($3)
          AND lifecycle_state = 'trashed'
        ",
    )
    .bind(node_id)
    .bind(restore_parent)
    .bind(subtree_ids)
    .execute(&mut **transaction)
    .await
    .map_err(|error| {
        if is_unique_violation(&error) {
            FolderError::NameConflict.into()
        } else {
            TrashMutationError::Database(error)
        }
    })?;
    Ok(())
}

async fn get_node_by_id_in_tx(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
) -> Result<Option<NodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, NodeRecord>(
        r"
        SELECT
            id, parent_id, name, kind, lifecycle_state,
            source_created_at, source_modified_at, created_at, updated_at
        FROM nodes
        WHERE id = $1
        ",
    )
    .bind(node_id)
    .fetch_optional(&mut **transaction)
    .await
}

async fn collect_active_subtree_ids(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    root_ids: &[Uuid],
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>(
        r"
        WITH RECURSIVE tree AS (
            SELECT id
            FROM nodes
            WHERE id = ANY($1)
              AND lifecycle_state = 'active'

            UNION

            SELECT child.id
            FROM nodes AS child
            JOIN tree AS parent ON child.parent_id = parent.id
            WHERE child.lifecycle_state = 'active'
        )
        SELECT id FROM tree
        ",
    )
    .bind(root_ids)
    .fetch_all(&mut **transaction)
    .await
}

async fn collect_trashed_subtree_ids(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    root_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>(
        r"
        WITH RECURSIVE tree AS (
            SELECT id
            FROM nodes
            WHERE id = $1
              AND lifecycle_state = 'trashed'

            UNION ALL

            SELECT child.id
            FROM nodes AS child
            JOIN tree AS parent ON child.parent_id = parent.id
            WHERE child.lifecycle_state = 'trashed'
        )
        SELECT id FROM tree
        ",
    )
    .bind(root_id)
    .fetch_all(&mut **transaction)
    .await
}

async fn ancestor_map_for_nodes(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, Vec<Uuid>>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid)>(
        r"
        WITH RECURSIVE ancestors AS (
            SELECT id AS node_id, parent_id AS ancestor_id
            FROM nodes
            WHERE id = ANY($1)

            UNION ALL

            SELECT a.node_id, n.parent_id
            FROM ancestors AS a
            JOIN nodes AS n ON n.id = a.ancestor_id
            WHERE a.ancestor_id IS NOT NULL
        )
        SELECT node_id, ancestor_id
        FROM ancestors
        WHERE ancestor_id IS NOT NULL
        ",
    )
    .bind(node_ids)
    .fetch_all(&mut **transaction)
    .await?;

    let mut map = std::collections::HashMap::<Uuid, Vec<Uuid>>::new();
    for (node_id, ancestor_id) in rows {
        map.entry(node_id).or_default().push(ancestor_id);
    }
    Ok(map)
}
