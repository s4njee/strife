//! `PostgreSQL` access and migration support for Strife.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, migrate::Migrator};
use strife_domain::{FolderError, FolderRules, NodeId};
use uuid::Uuid;

/// Embedded, versioned database migrations.
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
                   ), 0)
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
        JobType::Ocr => JobResourceClass::HeavyCpu,
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
    let candidate = sqlx::query_as::<_, JobRecord>(
        r"
        SELECT jobs.*
        FROM jobs
        WHERE job_type = $1 AND state = 'pending'
          AND (
            origin <> 'backfill'
            OR EXISTS (
                SELECT 1 FROM backfill_campaigns c
                WHERE c.id = jobs.campaign_id AND c.state = 'running'
                  AND (SELECT count(*) FROM jobs active
                       WHERE active.campaign_id = c.id AND active.state = 'leased') < c.max_running
            )
          )
        ORDER BY CASE
                     WHEN origin = 'backfill' AND $2 >= (
                         SELECT c.foreground_fairness
                         FROM backfill_campaigns c
                         WHERE c.id = jobs.campaign_id
                     ) THEN -1
                     ELSE CASE origin
                     WHEN 'foreground' THEN 0
                     WHEN 'repair' THEN 1
                     WHEN 'backfill' THEN 2
                     END
                 END,
                 priority DESC, created_at, id
        FOR UPDATE SKIP LOCKED
        LIMIT 1
        ",
    )
    .bind(job_type)
    .bind(foreground_claims)
    .fetch_optional(&mut *transaction)
    .await?;
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
        ON CONFLICT DO NOTHING
        ",
    )
    .bind(Uuid::new_v4())
    .bind(node_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
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
        INSERT INTO jobs (id, job_type, target_node_id, priority, max_attempts)
        VALUES ($1, 'ocr', $2, -10, 5)
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
                    SELECT array_agg(da.storage_key)
                    FROM derived_artifacts AS da
                    WHERE da.node_id = t.id
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
