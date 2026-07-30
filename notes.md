# Strife — Upload Flow: Key Files Reference

A file-by-file breakdown of every module involved in basic upload functionality across the frontend, backend, and database layers.

---

## Frontend (`apps/web/src/`)

### API Client & Types

| File | Role |
|---|---|
| `apps/web/src/api/client.ts` | HTTP client wrapping `fetch` for all upload endpoints: `createUploadSession` (`POST /api/uploads`), `uploadFileChunk` (`PATCH /api/uploads/:id` with `Content-Range`), `finalizeUpload` (`POST /api/uploads/:id/finalize`), `cancelUpload` (`DELETE /api/uploads/:id`), and `getActiveUploads` (`GET /api/uploads`). |
| `apps/web/src/api/types.ts` | TypeScript interfaces for upload domain models: `UploadSession`, `CreatedUploadSession`, `UploadSessionState`, and `UploadByteRange`. |

### Upload Engine (`apps/web/src/uploads/`)

| File | Role |
|---|---|
| `apps/web/src/uploads/UploadContext.tsx` | SolidJS context provider and `useUploads()` hook. Manages the reactive upload queue state, `AbortController` cancellation handles, server session discovery, and upload resumption with locally selected files. |
| `apps/web/src/uploads/folderUpload.ts` | Upload orchestration engine. Chunks files into configurable byte sizes (`DEFAULT_CHUNK_SIZE`), calculates missing byte ranges for resume, manages concurrent uploads (max 3), and recursively creates destination folder hierarchies via `ensureFolderPath`. |
| `apps/web/src/uploads/dropFiles.ts` | Parses browser `DataTransfer` drag-and-drop objects. Recursively traverses `FileSystemEntry` items (files and directories) to produce a flat `UploadCandidate` list with preserved relative paths. |
| `apps/web/src/uploads/uploadPersistence.ts` | IndexedDB persistence layer (`strife-uploads` database). Stores active `File` handles by upload session ID so interrupted uploads can be resumed across page reloads. |

### UI Components

| File | Role |
|---|---|
| `apps/web/src/components/FileUploadControl.tsx` | Multi-file picker button with a hidden `<input type="file" multiple>`. Collects selected files and initiates uploads into the current folder via `UploadContext`. |
| `apps/web/src/components/FolderUploadControl.tsx` | Folder picker button using a hidden `<input type="file" webkitdirectory>`. Uploads entire directory trees and displays an inline completion report with success/error counts. |
| `apps/web/src/components/UploadProgressPanel.tsx` | Floating bottom-right panel showing active upload items with progress bars, estimated remaining time, file re-selection pickers for resumed sessions, and cancel buttons. Mounted globally in `App.tsx` so it survives folder navigation. |
| `apps/web/src/components/StorageWarning.tsx` | Monitors disk usage via the readiness endpoint. Displays warning (≥80%) or error (≥90%) banners and disables upload controls when storage capacity is exceeded. |

### Views & App Shell

| File | Role |
|---|---|
| `apps/web/src/views/WorkspaceView.tsx` | Main workspace view. Attaches drag-and-drop event handlers (`dragover`, `dragleave`, `drop`) to the file browser area, invokes `collectDroppedFiles`, embeds `FileUploadControl` and `FolderUploadControl`, and triggers folder refetches on upload completion. |
| `apps/web/src/App.tsx` | Top-level application shell. Wraps the view tree in `UploadProvider` and mounts `UploadProgressPanel` globally so upload state is visible across all routes. |

### Styles

| File | Role |
|---|---|
| `apps/web/src/components/UploadDropZone.css` | Styles for the drag-and-drop target overlay border shown when files are dragged over the workspace. |
| `apps/web/src/components/FolderUploadControl.css` | Styles for the folder upload button and its floating completion status popover. |
| `apps/web/src/components/UploadProgressPanel.css` | Layout, fixed positioning, progress bar indicators, state colors, and scroll container styles for the upload progress panel. |

---

## Backend (`crates/`)

### API Layer (`crates/api/`)

| File | Role |
|---|---|
| `crates/api/src/uploads.rs` | Core upload HTTP endpoints: `POST /api/uploads` (initiate — validates folder, checks conflicts and disk guard, creates staging key and session), `PATCH /api/uploads/:id` (chunk — parses `Content-Range`, streams body to staging, computes SHA-256), `POST /api/uploads/:id/finalize` (verify bytes, detect MIME, atomically commit node + file_object + session, enqueue metadata job), `GET /api/uploads/:id` (progress for resume), `DELETE /api/uploads/:id` (cancel and delete staging). |
| `crates/api/src/lib.rs` | Configures and mounts the uploads router into the Axum app. Spawns a periodic Tokio background task (`spawn_upload_cleanup`) that sweeps and purges expired upload sessions and their staging files. |
| `crates/api/src/config.rs` | Reads upload-relevant environment settings: `UPLOAD_SESSION_TTL_HOURS` (session expiry), `DISK_GUARD_PERCENT` (storage capacity limit, default 90). |
| `crates/api/src/files.rs` | File download, metadata inspection, and preview serving for finalized uploads. Supports `Range` requests for `206 Partial Content` streaming. |
| `crates/api/src/storage_usage.rs` | Serves storage usage metrics, distinguishing between finalized file object sizes and active staging upload bytes. |

### Domain Layer (`crates/domain/`)

| File | Role |
|---|---|
| `crates/domain/src/lib.rs` | Defines domain primitives (`Node`, `NodeId`, `NodeKind`, `LifecycleState`) and stateless validation rules (`FolderRules::validate_name`, `FolderRules::validate_unique_sibling`) used to validate the target folder and display name during upload initiation. |

### Storage Layer (`crates/storage/`)

| File | Role |
|---|---|
| `crates/storage/src/lib.rs` | Defines the `StorageBackend` trait, storage namespaces (`Staging`, `Originals`, `Artifacts`), `DiskGuard`, `DiskUsage`, and the `LocalFsBackend` implementation. Key upload methods: `write_range` (streams a chunk to a staging file at a specific byte offset), `move_object` (atomically moves a file from staging to originals on finalization), and capacity checks for the disk guard. |

### Media Layer (`crates/media/`)

| File | Role |
|---|---|
| `crates/media/src/lib.rs` | Exports MIME detection using libmagic to identify file type from content bytes (not extension). Called during upload finalization to set the MIME type on the `file_object`. |

### Worker (`crates/worker/`)

| File | Role |
|---|---|
| `crates/worker/src/lib.rs` | Background worker runtime loop. Implements `claim_job` / `complete_job` cycle, worker configuration (`WorkerConfig`), and job dispatching to `MetadataHandler` for finalized uploads. |
| `crates/worker/src/metadata.rs` | Handles `MetadataExtraction` and `PreviewGeneration` jobs triggered after upload finalization. Runs ExifTool, ffprobe, and Tika extractors, updates `file_objects` with detected MIME, and generates derived preview artifacts. |

---

## Database (`crates/db/`)

### Migrations

| File | Role |
|---|---|
| `crates/db/migrations/0002_nodes.up.sql` | Creates the `node_kind` and `node_lifecycle_state` enums and the `nodes` table. Upload finalization creates a new `file` node here with the uploaded file's display name, parent folder, and preserved source timestamps. |
| `crates/db/migrations/0003_file_objects.up.sql` | Creates the `file_upload_state` enum (`staging`, `finalized`) and the `file_objects` table. Maps a node to its physical storage key, byte size, MIME type, SHA-256 checksum, and upload state. |
| `crates/db/migrations/0004_upload_sessions.up.sql` | Creates the `upload_session_state` enum, `upload_sessions` table (session metadata, received bytes, staging key, expiry), and `upload_chunks` table (per-session byte ranges for resumability). |
| `crates/db/migrations/0005_jobs.up.sql` | Creates the `job_type` and `job_state` enums and the `jobs` table. After upload finalization, a `metadata_extraction` job is enqueued here for background processing. |

### Data Access Layer

| File | Role |
|---|---|
| `crates/db/src/lib.rs` | Single-file data access layer containing all Rust models and SQLx query functions for the upload flow. Models: `UploadSessionRecord`, `UploadSessionState`, `FileObjectRecord`, `FileUploadState`, `UploadSessionProgress`, `ReceivedRange`, `CreateUploadSession`. Functions: `create_session`, `record_chunk`, `get_session_progress`, `update_received_bytes`, `finalize_upload`, `cancel_session`, `expire_session`, `create_file_object`, `finalize_file_object`. Manages the entire upload lifecycle transactionally — from chunk ingestion through node/file_object creation and metadata job enqueueing. |

### Tests

| File | Role |
|---|---|
| `crates/db/tests/upload_sessions.rs` | Integration tests for upload session creation, chunk recording, progress tracking, expiration, and cancellation. |
| `crates/db/tests/file_objects.rs` | Integration tests for creating, fetching, and updating `file_objects` records and transitioning their upload states. |
