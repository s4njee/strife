# Epic 3 — Watched-Folder Import


**Goal:** Files placed in the fixed server-side inbox are manually discovered, validated, and moved into Strife using the same finalization pipeline as uploads.

**Sprint Capacity Estimate:** 2 sprints

---

### Story 3.1 — Resolve Import Questions

As a developer, I want all Milestone 3 questions in [`questions.md`](../../questions.md) decided and recorded so that import behavior is unambiguous. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] Decisions recorded in `docs/decisions/` for: copy vs move, watch path → destination mapping, stability detection, post-import source handling, re-import/disappearance handling, conflict handling.
- [x] `questions.md` M3 section is cleared; `README.md` updated.

**Implementation report:** Recorded the fixed `/mnt/ext/watch` → root mapping, manual-scan workflow, move-after-finalization semantics, in-stream stability check, and persistent conflict errors in ADR 0004 and the product plan. Deferred configurable sources and automatic watching to v2+, and added a later story for a unified actionable Errors tab.

**New files:**

- `docs/decisions/0004-watched-folder-import.md`

**Modified files:**

- `README.md`
- `deferred.md`
- `questions.md`

---

### Story 3.2 — Import Source Schema

As a developer, I want `import_sources` and `import_entries` tables so that the fixed source and per-file state are durably tracked. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `import_sources` has: `id`, `watch_path` (text, unique), `destination_folder_id` (FK), `enabled` (bool), `last_scan_at`, `created_at`, `updated_at`, and seeds the fixed `/mnt/ext/watch` → root source.
- [x] `import_entries` has: `id`, `source_id` (FK), `source_path` (text), `source_size` (bigint), `source_modified_at`, `source_checksum` (text, nullable), `state` (enum: `discovered` | `stable` | `importing` | `imported` | `failed`), `resulting_node_id` (FK, nullable), `error_message` (text, nullable), `created_at`, `updated_at`.
- [x] A unique constraint on `(source_id, source_path)` prevents duplicate tracking of the same file.
- [x] DB queries: `upsert_import_entry`, `list_pending_entries`, `mark_imported`, `mark_failed`.

**Implementation report:** Added a migration for the fixed import source and durable per-file lifecycle, including constraints and a pending-work index. Added typed database records and idempotent discovery, pending-list, imported, and failed operations with a PostgreSQL lifecycle test.

**New files:**

- `crates/db/migrations/0006_import_sources.down.sql`
- `crates/db/migrations/0006_import_sources.up.sql`
- `crates/db/tests/import_entries.rs`

**Modified files:**

- `crates/db/src/lib.rs`

---

### Story 3.3 — File Discovery Scanner

As a user, I want a directory scanner in `crates/importer` so that files are detected when I request an import. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] A scanner function walks the configured `watch_path` recursively.
- [x] For each regular file found, it upserts an `import_entries` row with `state = discovered`, recording size and modification time.
- [x] Symbolic links, device files, sockets, and other special files are **skipped** (logged at debug level).
- [x] Hidden files (starting with `.`) are skipped by default (configurable).
- [x] Directories in the watch path are recorded so hierarchy can be recreated.
- [x] The scanner runs only when manually invoked.
- [x] The scanner is idempotent: re-scanning the same unchanged file does not create duplicate entries.
- [x] Tests: create files in a temp dir, run the scanner, verify entries are created.

**Implementation report:** Implemented a deterministic, manually invoked recursive scanner with a PostgreSQL discovery sink, parent-first directory reporting, configurable hidden-file inclusion, and debug logging for skipped entries. A temporary-tree test verifies nested discovery while hidden files and symlinks are excluded.

**New files:** None.

**Modified files:**

- `Cargo.lock`
- `crates/importer/Cargo.toml`
- `crates/importer/src/lib.rs`

---

### Story 3.4 — Stability Detection

As a system, I want to reject files that change while being staged so that partially written files are not imported. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] The file's size and modification time are captured immediately before and after streaming it into staging.
- [x] An unchanged file transitions to `state = stable` and may be finalized.
- [x] A changed or missing file returns to `state = discovered`; its staging object is deleted and the source remains untouched.
- [x] Only `stable` entries proceed to finalization.
- [x] Tests simulate a file changing during staging and verify it is not finalized or removed.

**Implementation report:** Added guarded staging that compares regular-file size and modification time before and after streaming, publishes only an unchanged staging object, and persists either `stable` or a reset to `discovered`. Tests cover successful staging and rejection after the discovery snapshot changes, with source files left untouched.

**New files:** None.

**Modified files:**

- `crates/db/src/lib.rs`
- `crates/importer/src/lib.rs`

---

### Story 3.5 — Import Pipeline

As a system, I want stable files processed through the same checksum/finalization pipeline as uploads so that imports are consistent and reliable. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] For each stable entry, the importer: checks disk guard (90%), checks name conflict at the destination, streams the file into staging via `StorageBackend`, computes SHA-256 during streaming, detects MIME, and atomically finalizes (creates node, file_object, marks entry as `imported`).
- [x] Source filesystem timestamps are preserved on the created node.
- [x] Hierarchy is preserved: if the source is `watch_path/photos/2024/img.jpg`, create folders `photos` and `2024` under the destination before importing `img.jpg`.
- [x] Folder creation reuses existing folders if they already exist (no conflict on pre-existing matching folder).
- [x] On conflict (duplicate file name), the entry is marked `failed` with a clear error message; it does **not** block other imports.
- [x] On completion, a metadata extraction job is enqueued and the source file is removed; empty source directories are pruned.
- [x] Tests: import a tree of 5 files across 3 directories; verify nodes, hierarchy, checksums, and no duplicates.

**Implementation report:** Implemented the watched-file ingestion pipeline with the shared disk guard, deterministic staging/original keys, streaming SHA-256, content MIME detection, and transactional folder/node/object/job/entry publication. Relative hierarchy and source timestamps are preserved, existing folders are reused, conflicts become persistent entry failures, and sources plus empty inbox directories are removed only after commit.

**New files:**

- `crates/importer/tests/import_pipeline.rs`

**Modified files:**

- `Cargo.lock`
- `crates/db/src/lib.rs`
- `crates/importer/Cargo.toml`
- `crates/importer/src/lib.rs`

---

### Story 3.6 — Import Restart & Idempotency

As a system, I want imports to survive service restarts without duplication so that the system is reliable. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] On startup, the importer loads all `import_entries` with state `importing` and retries them.
- [x] If a node was already created (crash after finalization but before marking `imported`), the retry detects it via the source path unique constraint and marks it `imported`.
- [x] If staging was written but not finalized, the retry re-finalizes.
- [x] No restart scenario creates a duplicate node for the same source file.
- [x] Test: simulate a crash mid-import (kill the process), restart, verify exactly one node exists.

**Implementation report:** Added a durable `importing` checkpoint and an API-startup recovery pass that retries interrupted entries independently using deterministic storage keys and the idempotent finalization transaction. Retries also recognize a previously published node and finish source cleanup without touching its managed original; the restart integration test runs recovery twice and verifies exactly one node is published.

**New files:** None.

**Modified files:**

- `crates/api/src/lib.rs`
- `crates/db/src/lib.rs`
- `crates/importer/src/lib.rs`
- `crates/importer/tests/import_pipeline.rs`

---

### Story 3.7 — Import Management API

As a user, I want API endpoints to configure and monitor watched-folder imports so that I can control imports without SSH-ing into the server. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `GET /api/import-sources` returns the fixed source with its status (enabled, last scan time, entry counts by state).
- [x] `PATCH /api/import-sources/:id` toggles `enabled`.
- [x] `POST /api/import-sources/:id/scan` validates that the fixed path exists, is readable, and does not overlap managed storage, then runs one scan/import pass.
- [x] `GET /api/import-sources/:id/entries?state=failed` lists entries filtered by state, with error messages.
- [x] `POST /api/import-sources/:id/entries/:entry_id/retry` resets a failed entry to `discovered` for re-processing.

**Implementation report:** Added fixed-source management endpoints for aggregate status, enable/disable, validated manual scans, state-filtered entries, and failed-entry retry. API integration tests cover the full manual scan flow, persistent name-conflict errors, retry reset, disabled-source rejection, and managed-storage overlap protection.

**New files:**

- `crates/api/src/imports.rs`
- `crates/api/tests/imports_api.rs`

**Modified files:**

- `Cargo.lock`
- `crates/api/Cargo.toml`
- `crates/api/src/lib.rs`
- `crates/db/src/lib.rs`

---

### Story 3.8 — Import Status UI

As a user, I want to see import progress and errors in the UI so that I know what's happening with my watched folder. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] A section in the sidebar or a dedicated page shows configured import sources.
- [x] Each source displays: watch path, destination, enabled/disabled, last scan time, counts (discovered / importing / imported / failed).
- [x] Failed entries are listed with their error messages and a "Retry" button.
- [x] A toggle to enable/disable the source is available.
- [x] The status refreshes periodically (every 30 seconds) or on user action.

**Implementation report:** Added an Imports sidebar destination and responsive status page with the fixed path/destination, live lifecycle counts, last scan time, manual scan and enable controls, plus actionable failed-entry retry. The page refreshes every 30 seconds and after mutations, and includes representative static fixture data for the GitHub Pages preview.

**New files:**

- `apps/web/src/views/ImportStatusView.css`
- `apps/web/src/views/ImportStatusView.tsx`

**Modified files:**

- `apps/web/src/App.css`
- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/components/Sidebar.css`
- `apps/web/src/components/Sidebar.tsx`
- `apps/web/src/index.tsx`

---
