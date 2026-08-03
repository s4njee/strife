# Epic 4 — Metadata Extraction


**Goal:** Uploaded and imported files have rich metadata extracted asynchronously via durable background jobs, without blocking ingestion.

**Sprint Capacity Estimate:** 3 sprints

---

### Story 4.1 — Resolve Metadata Questions

As a developer, I want M4 questions decided (raw retention, typed columns, format test matrix) so that metadata schema and extractor implementation are clear. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] Decisions recorded in `docs/decisions/` for: raw metadata size/retention policy, first-class typed columns, and the explicit format test matrix.
- [x] `questions.md` M4 section is cleared.

**Implementation report:** Recorded full raw-JSON retention with a 10–15 GB-per-million-file planning target, separated document/OCR text from metadata payloads, selected a one-to-one normalized metadata model plus per-stream fields, and established the explicit DOC/DOCX/PDF/JPEG/GIF/PNG/NEF/DNG/MP4/MKV/MOV/MP3/M4A acceptance matrix.

**New files:**

- `docs/decisions/0005-metadata-storage-and-format-matrix.md`

**Modified files:**

- `README.md`
- `questions.md`

---

### Story 4.2 — Jobs Schema & Queue

As a developer, I want a `jobs` table and a `FOR UPDATE SKIP LOCKED` job queue so that metadata and preview work is durable and retry-safe. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Migration creates `jobs` with: `id` (UUID), `job_type` (enum: `metadata_extraction` | `preview_generation` | `trash_cleanup` | `permanent_deletion`), `target_node_id` (FK), `state` (enum: `pending` | `leased` | `completed` | `failed` | `cancelled`), `priority` (int, default 0), `attempts` (int, default 0), `max_attempts` (int, default 3), `lease_owner` (text, nullable), `lease_expires_at` (timestamptz, nullable), `last_error` (text, nullable), `created_at`, `updated_at`, `completed_at`.
- [x] `claim_job(job_type, owner)` uses `SELECT ... FOR UPDATE SKIP LOCKED` to lease the highest-priority pending job, setting `lease_owner`, `lease_expires_at` (now + configurable TTL), and incrementing `attempts`.
- [x] `complete_job(id)` marks it `completed`.
- [x] `fail_job(id, error)` marks it `failed` if `attempts >= max_attempts`, otherwise resets to `pending` with the error recorded.
- [x] `release_expired_leases()` finds jobs where `lease_expires_at < now()` and resets them to `pending`.
- [x] Enqueueing the same `(job_type, target_node_id)` when one is already `pending` is a no-op (idempotent).
- [x] Tests: enqueue, claim, complete, fail with retry, expire lease.

**Implementation report:** Reused the Epic 2 jobs migration and added a typed PostgreSQL queue API with atomic priority leasing, configurable lease TTLs, idempotent enqueueing, retry exhaustion, completion, and expired-lease recovery. Integration coverage exercises the complete queue lifecycle against PostgreSQL.

**New files:**

- `crates/db/tests/jobs.rs`

**Modified files:**

- `crates/db/src/lib.rs`

---

### Story 4.3 — Worker Binary & Job Loop

As a developer, I want `crates/worker` to run a loop claiming and executing jobs so that metadata and previews are processed in the background. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `crates/worker` is a binary that connects to PostgreSQL and the storage backend.
- [x] It runs a configurable number of concurrent job processors (default: 2, tunable for 4 GB RAM via `WORKER_CONCURRENCY`).
- [x] Each processor loops: claim a job → execute the handler → complete/fail the job.
- [x] If no job is available, the processor sleeps for a configurable interval (default: 5s) before polling again.
- [x] A periodic task (every 60s) calls `release_expired_leases()`.
- [x] Structured JSON logging with a `job_id` correlation ID on every log line during processing.
- [x] Graceful shutdown on SIGTERM: finish current jobs, then exit.

**Implementation report:** Added the production worker runtime with environment-based PostgreSQL and storage configuration, bounded concurrent processors, durable complete/fail transitions, periodic lease recovery, and correlated JSON logs. SIGTERM and Ctrl-C stop new claims while allowing in-flight handlers to finish; the concrete metadata handler remains deliberately unavailable until Story 4.9 so queued work is never falsely completed.

**New files:**

- `crates/worker/src/lib.rs`

**Modified files:**

- `Cargo.toml`
- `Cargo.lock`
- `crates/worker/Cargo.toml`
- `crates/worker/src/main.rs`

---

### Story 4.4 — Metadata Schema

As a developer, I want `metadata_records` and `media_streams` tables so that extracted metadata is stored durably. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `metadata_records`: `id`, `node_id` (FK), `extractor_name` (text), `extractor_version` (text), `status` (enum: `pending` | `completed` | `failed` | `unsupported`), `raw_payload` (jsonb), `warnings` (text[]), `created_at`, `updated_at`.
- [x] `media_streams`: `id`, `node_id` (FK), `stream_index` (int), `stream_type` (enum: `video` | `audio` | `subtitle`), `codec` (text), `width` (int, nullable), `height` (int, nullable), `duration_ms` (bigint, nullable), `bitrate_bps` (bigint, nullable), `frame_rate` (text, nullable), `language` (text, nullable), `created_at`.
- [x] Add normalized typed columns to `nodes` or a separate `node_metadata` table (as decided in Story 4.1): `detected_mime`, `media_kind`, `duration_ms`, `width`, `height`, `capture_time`, `page_count`, `orientation`, `has_gps`.
- [x] Unique constraint on `(node_id, extractor_name)` — only one record per extractor per file.

**Implementation report:** Added the durable metadata schema for complete raw JSON payloads, extractor status and warnings, normalized file facts, and individual media streams. The schema also carries the additional camera, GPS, and document fields selected in Story 4.1, with cascading cleanup and integrity constraints.

**New files:**

- `crates/db/migrations/0007_metadata.up.sql`
- `crates/db/migrations/0007_metadata.down.sql`
- `crates/db/tests/metadata.rs`

**Modified files:**

- `Cargo.lock`
- `crates/db/Cargo.toml`

---

### Story 4.5 — libmagic / MIME Detection Adapter

As a developer, I want a MIME detection module in `crates/media` so that every file gets an accurate content-based MIME type. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] `crates/media` exports a `detect_mime(path: &Path) -> Result<String>` function.
- [x] Uses `libmagic` (via `tree_magic_mini`, `file` command, or an FFI binding) to detect MIME from the file's content bytes.
- [x] Falls back to `application/octet-stream` if detection fails.
- [x] Does **not** trust file extensions.
- [x] Tests: verify correct MIME for JPEG, PNG, PDF, MP4, MP3, DOCX, and an extensionless file.

**Implementation report:** Added a content-only MIME adapter backed by the host `file`/libmagic database, with a stable binary fallback when the detector is missing or rejects a file. Byte fixtures cover the required image, document, audio, video, OOXML, misleading-extension, and extensionless cases.

**New files:** None.

**Modified files:**

- `Cargo.lock`
- `crates/media/Cargo.toml`
- `crates/media/src/lib.rs`

---

### Story 4.6 — ExifTool Adapter

As a developer, I want an ExifTool adapter in `crates/media` so that images and raw files get rich EXIF/IPTC/XMP metadata. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `crates/media` exports an `extract_exif(path: &Path) -> Result<ExifResult>` function.
- [x] Invokes `exiftool -json -n <path>` as a child process.
- [x] Enforces a timeout (configurable, default: 30s) and kills the process if exceeded.
- [x] Enforces max output size (e.g., 5 MB) to prevent memory exhaustion.
- [x] Parses the JSON output into a structured `ExifResult` with normalized fields: `width`, `height`, `orientation`, `capture_time`, `camera_make`, `camera_model`, `gps_latitude`, `gps_longitude`, `color_space`.
- [x] Preserves the full raw JSON as `raw_payload` for storage in `metadata_records`.
- [x] Records warnings for missing or suspicious fields.
- [x] Tests with representative JPEG, PNG, and a raw camera file (e.g., CR2 or ARW).

**Implementation report:** Added a bounded asynchronous ExifTool adapter that preserves every successful JSON field while normalizing image, camera, capture-time, color, and GPS facts and flagging suspicious results. Its 16 MiB process ceiling fails oversized extraction atomically—never truncating stored metadata—and tests cover live JPEG/PNG extraction plus representative Nikon raw metadata.

**New files:**

- `crates/media/src/exif.rs`

**Modified files:**

- `Cargo.lock`
- `crates/media/Cargo.toml`
- `crates/media/src/lib.rs`

---

### Story 4.7 — ffprobe Adapter

As a developer, I want an ffprobe adapter in `crates/media` so that video and audio files get codec, stream, and duration metadata. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `crates/media` exports `extract_ffprobe(path: &Path) -> Result<FfprobeResult>`.
- [x] Invokes `ffprobe -v quiet -print_format json -show_format -show_streams <path>`.
- [x] Timeout: 60s (configurable). Max output: 5 MB.
- [x] Parses into `FfprobeResult` with: `container_format`, `duration_ms`, `total_bitrate`, and a `Vec<StreamInfo>` with per-stream `codec`, `type`, `width`, `height`, `frame_rate`, `bitrate`, `language`.
- [x] Populates `media_streams` table rows.
- [x] Tests with representative MP4 (H.264 + AAC), MKV, MP3, and M4A files.

**Implementation report:** Added a bounded `ffprobe` adapter that retains the complete JSON document and normalizes container, duration, bitrate, language, and per-stream fields into records shaped for `media_streams` persistence. Shared process controls enforce atomic 16 MiB/60-second extraction, and generated H.264/AAC MP4, MKV, MP3, and M4A fixtures validate real probes.

**New files:**

- `crates/media/src/ffprobe.rs`
- `crates/media/src/process.rs`

**Modified files:**

- `crates/db/src/lib.rs`
- `crates/db/tests/metadata.rs`
- `crates/media/src/exif.rs`
- `crates/media/src/lib.rs`

---

### Story 4.8 — Apache Tika Adapter

As a developer, I want a Tika adapter in `crates/media` so that PDFs and office documents get title, author, page count, and other properties. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `crates/media` exports `extract_tika(path: &Path, tika_url: &str) -> Result<TikaResult>`.
- [x] Sends the file to Tika's `/meta` endpoint via HTTP PUT with `Accept: application/json`.
- [x] Timeout: 60s. Max response: 5 MB.
- [x] Parses into `TikaResult` with normalized fields: `title`, `author`, `creation_date`, `modification_date`, `page_count`, `word_count`.
- [x] Preserves full Tika JSON as `raw_payload`.
- [x] Tests with representative PDF and DOCX files.

**Implementation report:** Added a streaming Apache Tika HTTP adapter with a 60-second timeout and atomic 16 MiB response ceiling, preserving every metadata property while normalizing common document facts. A local protocol-level test verifies PDF and DOCX uploads use `PUT /meta`, request JSON, parse scalar/array variants, and retain unknown raw fields.

**New files:**

- `crates/media/src/tika.rs`

**Modified files:**

- `Cargo.toml`
- `Cargo.lock`
- `crates/media/Cargo.toml`
- `crates/media/src/lib.rs`

---

### Story 4.9 — Metadata Extraction Job Handler

As a developer, I want the worker to handle `metadata_extraction` jobs so that metadata is extracted for every ingested file. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] When a `metadata_extraction` job is claimed, the handler: retrieves the file from storage to a temp path, runs MIME detection, selects the appropriate extractor(s) based on MIME (exiftool for images, ffprobe for video/audio, tika for documents), runs the extractor(s), inserts `metadata_records` and `media_streams` rows, updates normalized columns on the node, and marks the job completed.
- [x] If the MIME type doesn't match any specialized extractor, a generic `metadata_records` row is created with `status = unsupported` containing only MIME, size, and checksum.
- [x] If an extractor fails, the job is marked failed with the error; the file remains accessible with whatever metadata was extracted.
- [x] Extractor concurrency is bounded (max 1 ExifTool + 1 ffprobe + 1 Tika at a time, configurable) to respect 4 GB RAM.
- [x] Tests: enqueue a job for a JPEG, process it, verify `metadata_records` and normalized fields.

**Implementation report:** Wired MIME, ExifTool, ffprobe, and Tika into the durable worker with streamed temporary retrieval, independent extractor semaphores, full raw-record upserts, normalized facts, media stream replacement, and failure records that drive queue retry without affecting file access. A PostgreSQL/storage integration test processes a real generated JPEG through a leased job and verifies completion, full Exif JSON, MIME, width, and height.

**New files:**

- `crates/worker/src/metadata.rs`
- `crates/worker/tests/metadata_job.rs`

**Modified files:**

- `.env.example`
- `Cargo.lock`
- `crates/worker/Cargo.toml`
- `crates/worker/src/lib.rs`
- `crates/worker/src/main.rs`

---

### Story 4.10 — Gradual Reprocessing

As a developer, I want a mechanism to re-extract metadata when an extractor version changes so that old files benefit from improved extractors. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] A function `enqueue_reprocessing(extractor_name, old_version)` creates low-priority `metadata_extraction` jobs for all files whose `metadata_records` for that extractor have a version less than the current.
- [x] Reprocessing jobs have lower priority than new-file metadata jobs.
- [x] Reprocessing runs gradually (e.g., max 10 jobs enqueued at a time) to avoid flooding the queue.
- [x] The reprocessing is idempotent: running it twice doesn't create duplicate jobs.
- [x] Can be triggered via an internal API or admin endpoint: `POST /api/admin/reprocess?extractor=exiftool`.

**Implementation report:** Added gradual extractor-version reprocessing that selects at most ten stale records per call, enqueues them at priority -100, and relies on active-job uniqueness for repeat-safe operation. The internal admin endpoint supports the three specialized extractors and rejects unknown names.

**New files:**

- `crates/api/src/admin.rs`
- `crates/db/tests/reprocessing.rs`

**Modified files:**

- `crates/api/src/lib.rs`
- `crates/db/src/lib.rs`

---

### Story 4.11 — Metadata & Details API

As an API client, I want endpoints to retrieve file metadata and processing status so that the UI can display detailed file information. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `GET /api/files/:node_id` returns the node with all normalized metadata fields plus `processing_status` (derived from job state: `processing`, `ready`, `partially_processed`, `failed`).
- [x] `GET /api/files/:node_id/metadata` returns all `metadata_records` for the file (raw payloads included or excluded via a `?raw=true` query param).
- [x] `GET /api/files/:node_id/streams` returns `media_streams` for video/audio files.
- [x] GPS coordinates are included when available; no location reverse-geocoding in v1.

**Implementation report:** Expanded the files API with normalized detail, extractor record, and media stream endpoints, including opt-in raw JSON and exact GPS coordinates. Processing status combines active/failed jobs with successful/failed extractor records to distinguish processing, ready, partial, and failed files.

**New files:** None.

**Modified files:**

- `crates/api/Cargo.toml`
- `crates/api/src/files.rs`
- `crates/api/tests/files_api.rs`

---

### Story 4.12 — File Details Panel UI

As a user, I want a details side panel showing file metadata so that I can inspect file properties without opening a separate page. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Selecting a single file and clicking "Details" (or pressing a shortcut) opens a right-side panel.
- [x] The panel displays: file name, type icon, size, MIME type, created/modified dates, checksum (truncated with copy button).
- [x] For images: dimensions, orientation, camera make/model, capture time, GPS coordinates (if present).
- [x] For video/audio: duration, codec(s), resolution, bitrate, stream list.
- [x] For documents: title, author, page count, creation/modification dates.
- [x] Processing status is shown with an appropriate indicator (spinner for `processing`, checkmark for `ready`, warning for `failed`).
- [x] The panel works in both themes.
- [x] Closing the panel or selecting a different file updates the content.

**Implementation report:** Added a responsive, theme-token-driven file details drawer connected to single-row selection and the metadata/stream APIs, with type-specific image, media, and document sections, processing indicators, and checksum copying. Static preview fixtures now include document, camera/GPS image, and multi-stream video examples so the full Epic 4 UI can be reviewed on GitHub Pages.

**New files:**

- `apps/web/src/components/FileDetailsPanel.tsx`
- `apps/web/src/components/FileDetailsPanel.css`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/components/FileTable.tsx`
- `apps/web/src/components/FileTable.css`
- `apps/web/src/views/WorkspaceView.tsx`

---
