# Epic 2 — Resumable Upload & Download


**Goal:** Files can be uploaded (resumably, with chunking), downloaded, and range-streamed. Uploads survive page reloads and service restarts.

**Sprint Capacity Estimate:** 3–4 sprints

---

### Story 2.1 — Storage Backend Abstraction

As a developer, I want a `StorageBackend` trait in `crates/storage` with implementations for the chosen backend so that file I/O is decoupled from the rest of the application. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `crates/storage` defines a `StorageBackend` trait with async methods: `put_stream(key, stream) -> Result<()>`, `get_stream(key) -> Result<impl AsyncRead>`, `get_range(key, offset, length) -> Result<impl AsyncRead>`, `delete(key) -> Result<()>`, `exists(key) -> Result<bool>`, `disk_usage() -> Result<DiskUsage>`.
- [x] A `LocalFsBackend` (or `MinioBackend`) implements the trait using the decided storage approach.
- [x] Storage keys are opaque UUIDs — display names are **never** used as file paths.
- [x] Three separate namespaces (directories or prefixes) exist: `staging/`, `originals/`, `artifacts/`.
- [x] `put_stream` writes atomically: write to a temp file, then rename (or use MinIO's multipart upload).
- [x] Integration tests verify put/get/delete round-trip and that `get_range` returns correct byte ranges.
- [x] `disk_usage()` returns total, used, and available bytes for the storage volume.

**Implementation report:** Added an object-safe asynchronous storage contract and local-filesystem implementation with strongly typed UUID keys, isolated staging/originals/artifacts namespaces, and atomic temporary-file publication. Integration tests verify full round trips, exact ranged reads, idempotent deletion, namespace creation, and consistent total/used/available capacity reporting.

**New files:**

- `crates/storage/tests/local_fs.rs`

**Modified files:**

- `Cargo.lock`
- `Cargo.toml`
- `crates/storage/Cargo.toml`
- `crates/storage/src/lib.rs`

---

### Story 2.2 — File Object Schema

As a developer, I want the `file_objects` table linked to `nodes` so that uploaded file metadata is persisted alongside the hierarchy. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] Migration creates `file_objects` with columns: `id` (UUID PK), `node_id` (FK to `nodes`, unique for finalized objects), `storage_key` (text, not null), `byte_size` (bigint, not null), `mime_type` (text), `checksum_sha256` (text), `upload_state` (enum: `staging` | `finalized`), `created_at`, `updated_at`.
- [x] A constraint ensures a finalized node has exactly one finalized `file_object`.
- [x] DB query functions: `create_file_object`, `finalize_file_object`, `get_file_object_by_node_id`.

**Implementation report:** Added the `file_objects` migration, typed staged/finalized records, and create/finalize/get database queries with nonnegative-size and finalized-node constraints. A live PostgreSQL integration test confirms staged-to-finalized transitions and database rejection of a second finalized object for the same node.

**New files:**

- `crates/db/migrations/0003_file_objects.down.sql`
- `crates/db/migrations/0003_file_objects.up.sql`
- `crates/db/tests/file_objects.rs`

**Modified files:**

- `crates/db/src/lib.rs`

---

### Story 2.3 — Upload Session Schema

As a developer, I want the `upload_sessions` table to track resumable uploads so that chunk progress is durable across reloads and restarts. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] Migration creates `upload_sessions` with columns: `id` (UUID PK), `target_folder_id` (FK to `nodes`), `display_name` (text), `expected_byte_size` (bigint, nullable), `received_bytes` (bigint, default 0), `staging_key` (text), `state` (enum: `active` | `finalizing` | `completed` | `cancelled` | `expired`), `checksum_sha256` (text, nullable), `source_created_at` (timestamptz, nullable), `source_modified_at` (timestamptz, nullable), `expires_at` (timestamptz), `created_at`, `updated_at`.
- [x] A separate `upload_chunks` table (or `received_ranges` jsonb column) tracks which byte ranges have been received.
- [x] DB query functions: `create_session`, `record_chunk`, `get_session_progress`, `finalize_session`, `cancel_session`, `list_expired_sessions`.

**Implementation report:** Added durable upload-session and chunk-range schemas with lifecycle, expiry, active-name uniqueness, completed-node linkage, and ordered byte-range tracking. Typed queries and live PostgreSQL tests cover session creation, out-of-order non-overlapping chunks, atomic byte totals, overlap rejection, progress retrieval, completion, idempotent cancellation, and expiry listing.

**New files:**

- `crates/db/migrations/0004_upload_sessions.down.sql`
- `crates/db/migrations/0004_upload_sessions.up.sql`
- `crates/db/tests/upload_sessions.rs`

**Modified files:**

- `crates/db/src/lib.rs`

---

### Story 2.4 — Upload Initiation Endpoint

As an API client, I want `POST /api/uploads` to create an upload session so that I get a session ID to send chunks to. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `POST /api/uploads` accepts `{ "folder_id": UUID, "name": string, "size": number | null, "source_created_at": string | null, "source_modified_at": string | null }`.
- [x] Validates: folder exists and is active, no sibling name conflict among active nodes **and** other active upload sessions, disk usage is below 90% (or below 90% + declared size if size is known).
- [x] Creates a staging storage key and an `upload_sessions` row.
- [x] Returns `201` with `{ "session_id": UUID, "staging_key": string }`.
- [x] Returns `409` on name conflict, `507` on disk full, `404` if folder doesn't exist.
- [x] Session expires after a configurable TTL (default: 24 hours).

**Implementation report:** Added upload initiation with active-folder/name validation, UUID staging keys, projected capacity enforcement, source timestamp capture, and configurable 24-hour session expiry. Live PostgreSQL API tests with a mock storage backend verify creation plus duplicate-name, missing-folder, and insufficient-storage responses.

**New files:**

- `crates/api/src/uploads.rs`
- `crates/api/tests/uploads_api.rs`

**Modified files:**

- `.env.example`
- `Cargo.lock`
- `crates/api/Cargo.toml`
- `crates/api/src/config.rs`
- `crates/api/src/lib.rs`
- `crates/db/src/lib.rs`

---

### Story 2.5 — Chunk Upload Endpoint

As an API client, I want `PATCH /api/uploads/:session_id` to upload a chunk with a byte range so that large files can be uploaded incrementally. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Accepts a `Content-Range` header (e.g., `bytes 0-1048575/10485760`) and the chunk body.
- [x] Streams the chunk body to the staging file at the correct offset — does **not** buffer the entire chunk in memory.
- [x] Updates `upload_sessions.received_bytes` and records the range in `upload_chunks`.
- [x] Returns `200` with current progress: `{ "received_bytes": number, "expected_bytes": number | null, "complete": boolean }`.
- [x] Rejects overlapping or duplicate ranges with `409`.
- [x] Rejects chunks for non-active sessions with `404` or `410 Gone`.
- [x] Incrementally computes SHA-256 checksum as chunks arrive (or on finalization).
- [x] Handles out-of-order chunks correctly.

**Implementation report:** Added strict `Content-Range` parsing and streamed random-access writes that feed Axum bodies directly into staging files without whole-chunk buffering, with checksum calculation intentionally assigned to finalization. Unit and live integration tests cover invalid ranges, reversed chunk order, exact reconstructed bytes, progress totals, overlap rejection, and inactive-session `410` responses.

**New files:**

- None.

**Modified files:**

- `Cargo.lock`
- `Cargo.toml`
- `crates/api/Cargo.toml`
- `crates/api/src/uploads.rs`
- `crates/api/tests/uploads_api.rs`
- `crates/storage/src/lib.rs`

---

### Story 2.6 — Upload Finalization Endpoint

As an API client, I want `POST /api/uploads/:session_id/finalize` to commit the upload so that the file becomes a real node in the hierarchy. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Verifies all bytes are received (if `expected_byte_size` was set, `received_bytes` matches).
- [x] Computes (or finalizes) the SHA-256 checksum over the staging file.
- [x] Detects MIME type from file content bytes (using `libmagic` / `file` command), **not** from the file extension.
- [x] In a single transaction: moves the staging file to `originals/`, creates a `nodes` row (kind = `file`), creates a finalized `file_objects` row, updates the session to `completed`, and enqueues a metadata extraction job.
- [x] Re-checks name conflict at finalization time (another upload may have raced).
- [x] Returns `200` with the created node.
- [x] Returns `409` on name conflict, `400` if bytes are incomplete.
- [x] The operation is idempotent: calling finalize on an already-completed session returns the existing node.
- [x] Source timestamps (`source_created_at`, `source_modified_at`) from the session are preserved on the node.

**Implementation report:** Added recoverable storage promotion and a transactional finalization pipeline that verifies completeness, streams SHA-256, detects MIME from content, creates the file node/object, completes the session, and enqueues metadata extraction together. Live integration tests verify source timestamps, content-aware MIME, checksum, staging/original transitions, incomplete and raced-conflict handling, rollback to staging, and idempotent responses.

**New files:**

- `crates/db/migrations/0005_jobs.down.sql`
- `crates/db/migrations/0005_jobs.up.sql`

**Modified files:**

- `Cargo.lock`
- `Cargo.toml`
- `crates/api/Cargo.toml`
- `crates/api/src/uploads.rs`
- `crates/api/tests/uploads_api.rs`
- `crates/db/src/lib.rs`
- `crates/storage/src/lib.rs`

---

### Story 2.7 — Upload Cancellation & Cleanup

As an API client or system operator, I want to cancel an upload and have stale sessions cleaned up automatically so that staging space is reclaimed. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `DELETE /api/uploads/:session_id` cancels an active session, marks it `cancelled`, and deletes the staging file.
- [x] A background task (in `crates/worker` or the API process) runs periodically (e.g., every 15 minutes) to find expired sessions (`expires_at < now()`), delete their staging files, and mark them `expired`.
- [x] Cancellation and cleanup are idempotent.
- [x] Tests verify that a cancelled/expired session's staging file is removed from disk.

**Implementation report:** Added an idempotent cancellation endpoint and an API-process cleanup loop that sweeps every 15 minutes, removing expired staging objects before committing terminal session state so failed deletions can retry. Live integration tests verify repeated cancellation and cleanup calls, both terminal states, and physical staging-file removal.

**New files:**

- None.

**Modified files:**

- `crates/api/src/lib.rs`
- `crates/api/src/uploads.rs`
- `crates/api/tests/uploads_api.rs`
- `crates/db/src/lib.rs`

---

### Story 2.8 — Upload Progress Query

As an API client, I want `GET /api/uploads/:session_id` to check upload progress so that the UI can resume from where it left off after a reload. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] Returns `{ "session_id", "state", "display_name", "received_bytes", "expected_bytes", "received_ranges": [...], "created_at", "expires_at" }`.
- [x] `GET /api/uploads?folder_id=:id` lists all active sessions for a folder.
- [x] Used by the frontend to detect in-progress uploads on page load and resume them.

**Implementation report:** Added typed detail and folder-scoped active-upload queries with ordered received ranges and durable lifecycle timestamps, plus a SolidJS loader that discovers resumable sessions whenever a folder view loads. Integration tests verify the complete response contract, range ordering, and that completed sessions leave the active listing.

**New files:**

- None.

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/views/WorkspaceView.tsx`
- `crates/api/src/uploads.rs`
- `crates/api/tests/uploads_api.rs`
- `crates/db/src/lib.rs`

---

### Story 2.9 — File Download & Range Requests

As a user, I want to download a file and have video/audio seek via HTTP ranges so that I can retrieve my files and stream media. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `GET /api/files/:node_id/download` returns the original file with correct `Content-Type`, `Content-Length`, and `Content-Disposition: attachment; filename="<display_name>"` headers.
- [x] If the request includes a `Range` header, respond with `206 Partial Content`, `Content-Range`, and the requested byte range.
- [x] Support multi-range requests (or at minimum single-range).
- [x] Stream the file from storage — do **not** load the entire file into memory.
- [x] Return `404` for non-existent or trashed nodes.
- [x] Tests verify full download, single range, and that the downloaded content matches the uploaded content byte-for-byte.

**Implementation report:** Added a dedicated original-file download route that streams full bodies or closed, open-ended, and suffix byte ranges with safe attachment headers and standards-compliant 206/416 responses. Live filesystem/PostgreSQL tests compare full and partial bytes exactly and verify missing and trashed files remain inaccessible.

**New files:**

- `crates/api/src/files.rs`
- `crates/api/tests/files_api.rs`

**Modified files:**

- `crates/api/src/lib.rs`
- `crates/db/src/lib.rs`

---

### Story 2.10 — Folder Upload & Hierarchy Preservation

As a user, I want to upload an entire folder and have its directory structure preserved so that I don't have to recreate my folder hierarchy manually. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] The frontend reads `webkitRelativePath` from the `File` objects when a folder is selected via `<input webkitdirectory>`.
- [x] Before uploading, the client resolves the relative paths and issues `POST /api/folders` calls to create any missing intermediate folders.
- [x] Each file upload session references its correct parent folder.
- [x] If any folder creation or file upload fails due to a name conflict, the error is reported per-item; other non-conflicting items continue uploading.
- [x] The final folder structure in Strife mirrors the original on-disk structure.
- [x] Test: upload a folder with 3 levels of nesting and verify the hierarchy in the API.

**Implementation report:** Added a `webkitdirectory` folder picker and relative-path uploader that resolves or creates each intermediate folder, streams files into their exact parents, and isolates errors per file. A live three-level API test and browser run verified mirrored nesting, a 3-of-3 initial upload, and a repeat upload where three conflicts were reported while one new deep file still completed.

**New files:**

- `apps/web/src/components/FolderUploadControl.css`
- `apps/web/src/components/FolderUploadControl.tsx`
- `apps/web/src/uploads/folderUpload.ts`
- `crates/api/tests/folder_upload_api.rs`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/components/CreateFolderDialog.css`
- `apps/web/src/views/WorkspaceView.tsx`

---

### Story 2.11 — Disk Guard

As a system, I want upload initiation rejected when disk usage ≥ 90% so that the server doesn't fill up and crash. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] Before creating an upload session, check `storage.disk_usage()`.
- [x] If usage ≥ 90%, return `507 Insufficient Storage` with `{ "error": "disk_full", "usage_percent": number }`.
- [x] The 90% threshold is a configurable environment variable (`DISK_GUARD_PERCENT`, default 90).
- [x] The same check runs before watched-folder imports (Epic 3).
- [x] Test with a mock that simulates 91% usage and verify rejection.

**Implementation report:** Extracted a shared projected-capacity guard used by upload initiation and the watched-folder importer boundary, with a validated `DISK_GUARD_PERCENT` setting that defaults to 90. Unit and live API tests verify threshold projections and the exact 507 payload at simulated 91% usage.

**New files:**

- None.

**Modified files:**

- `.env.example`
- `Cargo.lock`
- `crates/api/src/config.rs`
- `crates/api/src/lib.rs`
- `crates/api/src/uploads.rs`
- `crates/api/tests/uploads_api.rs`
- `crates/importer/Cargo.toml`
- `crates/importer/src/lib.rs`
- `crates/storage/src/lib.rs`

---

### Story 2.12 — Upload UI — File Picker & Drag-Drop

As a user, I want to upload files via a file picker button and drag-and-drop so that uploading is fast and intuitive. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] An "Upload" button in the toolbar opens the native file picker (supports multi-select).
- [x] A second option or mode allows folder selection (`webkitdirectory`).
- [x] Dragging files/folders onto the table area shows a visual drop zone overlay.
- [x] Dropping files initiates upload sessions for each file.
- [x] Files are chunked client-side (default chunk size: 1 MB, configurable).
- [x] Each chunk is uploaded via `PATCH /api/uploads/:session_id` with the correct `Content-Range`.
- [x] Concurrent uploads are limited (e.g., max 3 simultaneous file uploads).

**Implementation report:** Added multi-file picking and directory-aware drag-and-drop with a themed table overlay, per-item results, a configurable 1 MiB chunk size, and a shared three-worker upload queue used by both file and folder flows. A live browser upload of a 2,349,483-byte file produced three persisted ranges spanning byte 0 through 2,349,482 and appeared in the table after finalization.

**New files:**

- `apps/web/src/components/FileUploadControl.tsx`
- `apps/web/src/components/UploadDropZone.css`
- `apps/web/src/uploads/dropFiles.ts`

**Modified files:**

- `.env.example`
- `apps/web/src/uploads/folderUpload.ts`
- `apps/web/src/views/WorkspaceView.tsx`

---

### Story 2.13 — Upload UI — Progress, Resume & Cancel

As a user, I want to see upload progress, resume after a reload, and cancel uploads so that I have full control over ongoing uploads. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] A persistent upload progress panel (bottom of the screen or a drawer) shows all active uploads with: file name, progress bar (percentage), bytes uploaded / total, estimated time remaining, and a cancel button.
- [x] On page load, the app queries `GET /api/uploads?folder_id=...` for active sessions and resumes them automatically.
- [x] Resuming: the app reads `received_ranges` from the session, identifies missing byte ranges, and uploads only those ranges.
- [x] Clicking "Cancel" calls `DELETE /api/uploads/:session_id` and removes the item from the progress panel.
- [x] When an upload completes, the file appears in the table immediately (optimistic update or refetch).
- [x] Conflict errors are displayed inline per-file in the progress panel.
- [x] The progress panel survives navigation between folders (it's outside the route content area).

**Implementation report:** Added a route-independent upload context and persistent panel with per-file progress, byte totals, ETA, cancel/resume controls, inline failures, immediate folder refresh, and IndexedDB-backed source retention for automatic reload recovery. Browser tests verified the panel across navigation, real cancellation, and a resumed 23-byte upload that preserved received range `0–5`, sent only `6–22`, completed, and appeared immediately in the table.

**New files:**

- `apps/web/src/components/UploadProgressPanel.css`
- `apps/web/src/components/UploadProgressPanel.tsx`
- `apps/web/src/uploads/UploadContext.tsx`
- `apps/web/src/uploads/uploadPersistence.ts`

**Modified files:**

- `apps/web/src/App.tsx`
- `apps/web/src/api/client.ts`
- `apps/web/src/components/FileUploadControl.tsx`
- `apps/web/src/components/FolderUploadControl.tsx`
- `apps/web/src/uploads/folderUpload.ts`
- `apps/web/src/views/WorkspaceView.tsx`

---

### Story 2.14 — Low Disk Warning UI

As a user, I want a persistent notification when disk usage is high so that I know to free space before uploads start failing. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] The app periodically checks `GET /api/ready` (or a dedicated endpoint) for `disk_usage_percent`.
- [x] When usage ≥ 80%, a persistent warning banner appears at the top of the content area: "Storage is almost full (X% used)".
- [x] When usage ≥ 90%, the banner becomes an error state: "Storage is full. Uploads and imports are disabled."
- [x] The banner is not dismissible while the condition persists.
- [x] When usage drops below 80%, the banner disappears.

**Implementation report:** Added a route-persistent content banner that reads readiness immediately and every 60 seconds, staying hidden below 80%, warning with the live percentage from 80–89%, and switching to a non-dismissible alert at 90% or above. Browser checks against 84% and 92% preview fixtures verified the exact warning and error copy, semantic status roles, and themed presentation.

**New files:**

- `apps/web/src/components/StorageWarning.css`
- `apps/web/src/components/StorageWarning.tsx`

**Modified files:**

- `.env.example`
- `apps/web/src/App.tsx`
- `apps/web/src/styles/tokens.css`

---
