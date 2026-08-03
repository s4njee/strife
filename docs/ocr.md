# Strife — OCR & Document Text Scrum Epics & Stories

> Derived from the **OCR and Text Extraction** section of [`deferred.md`](../deferred.md), whose questions are now answered. Epic numbering continues from [`scrum.md`](../scrum.md), which ends at Epic 12. Stories are ordered by dependency within each epic. Point estimates use a Fibonacci scale (1, 2, 3, 5, 8, 13). Acceptance criteria are written so a mid-level dev can implement and self-verify without ambiguity.

**Answers carried in from `deferred.md`:** English only initially · global (not per-file) language selection · PDF, JPEG, PNG, TIFF, WebP, and raw images as inputs · automatic OCR for all supported inputs · PDFs with any embedded text skip OCR and store that text instead · limits profiled later, sensible defaults now · all text stored in PostgreSQL for cross-drive search rather than as searchable-PDF derivatives · text visible and copyable in the UI · page/segment boundaries, language, and confidence retained · page-level confidence and warnings surfaced · text read-only and regenerable · no handwriting recognition · manual OCR re-trigger required · a dedicated OCR page reachable from the sidebar with counts, status, and an SSE console.

---

## Epic 13 — OCR Decisions & Text Storage Foundation

**Goal:** The OCR answers in `deferred.md` become recorded decisions, and PostgreSQL gains the text tables and job type that every later story writes into.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 13.1 — Resolve OCR Questions & Record Decisions

As a developer, I want the answered OCR questions recorded as an Architecture Decision Record so that downstream stories have fixed constraints. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] `docs/decisions/0008-ocr-and-document-text.md` follows the existing ADR shape (context, decision, alternatives, consequences, date) used by `docs/decisions/0004-watched-folder-import.md`.
- [x] The decision records: English-only initial language, global language configuration, the input format list, automatic processing for all supported inputs, embedded-PDF-text precedence over OCR, PostgreSQL text storage instead of OCRmyPDF derivatives, read-only text, no handwriting recognition, and manual re-trigger.
- [x] The decision explicitly rejects generating searchable-PDF derivatives and states why (the goal is cross-drive search, not per-file downloads), so the option is not silently revisited.
- [x] The **OCR and Text Extraction** section is removed from `deferred.md`, and its two remaining open items — concrete resource limits and reprocessing-on-version-change policy — are restated as explicit follow-ups rather than dropped.
- [x] `README.md` references the new decision alongside the existing ADR list.
- [x] The decision resolves the **implicit OCR already happening today**: `docs/tika.md` records that Tika's PDF parser may invoke its bundled Tesseract on scanned PDFs according to Tika's own defaults, that this explains `tesseract` consuming CPU on the Pi, and that the recognized text is then discarded. The ADR states whether that implicit path is disabled in favour of the explicit pipeline, or captured and reused — leaving it as-is means every scanned PDF is OCR'd twice.
- [x] `docs/tika.md` is updated: its opening claim that Strife "does **not** currently provide stored document text, OCR text, or document-content search" and its "Deliberately absent today" list both become wrong once this work lands.

**Implementation report:** Recorded the English-first explicit OCR contract, PostgreSQL text provenance, embedded-PDF precedence, read-only UI boundary, character-weighted confidence, manual reprocessing policy, and provisional resource-limit posture in ADR 0008. Removed the answered deferred questions, retained the two operational follow-ups, and documented that metadata-only Tika requests must disable implicit OCR so scanned PDFs are not processed twice.

**New files:**

- `docs/decisions/0008-ocr-and-document-text.md`

**Modified files:**

- `README.md`
- `deferred.md`
- `docs/ocr.md`
- `docs/tika.md`

---

### Story 13.2 — Document Text Schema

As a developer, I want durable tables for extracted text so that OCR output and embedded PDF text share one queryable representation. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] A migration pair `crates/db/migrations/0013_document_text.{up,down}.sql` is added, continuing the numbering after `0012_import_scan_jobs`.
- [x] `CREATE TYPE document_text_source AS ENUM ('embedded', 'ocr')` distinguishes text lifted from a PDF's own text layer from text produced by the OCR engine.
- [x] `CREATE TYPE document_text_status AS ENUM ('pending', 'completed', 'failed', 'skipped', 'unsupported')` mirrors the `metadata_status` vocabulary already in `0007_metadata.up.sql`.
- [x] `document_text` has: `node_id` (PK, FK to `nodes` with `ON DELETE CASCADE`), `source`, `status`, `language` (text), `engine_name`, `engine_version`, `page_count` (int, nullable), `mean_confidence` (real, nullable), `char_count` (int), `warnings` (`TEXT[]` defaulting to `'{}'`), `duration_ms` (bigint, nullable), `created_at`, `updated_at`.
- [x] `document_text_pages` has: `id`, `node_id` (FK, cascade), `page_number` (int, 1-based), `content` (text), `confidence` (real, nullable), `width`, `height`, with `UNIQUE (node_id, page_number)` — this is what preserves the page boundaries the answers require.
- [x] `mean_confidence` and `confidence` carry a `CHECK` constraint restricting them to `0.0 … 100.0`, matching Tesseract's reported scale.
- [x] Deleting a node removes its text rows; a test asserts the cascade, matching the pattern in `crates/db/tests/import_entries.rs`.
- [x] DB functions in `crates/db/src`: `upsert_document_text`, `replace_document_text_pages`, `get_document_text`, `list_document_text_pages`, `count_document_text_by_status`.
- [x] `replace_document_text_pages` is transactional and idempotent: re-running OCR for a node replaces every page atomically rather than accumulating duplicates.
- [x] A PostgreSQL integration test covers insert, replace, cascade delete, and status counting.

**Implementation report:** Added the reversible document-text schema with explicit provenance and lifecycle enums, document- and page-level constraints, cascade ownership, and indexed status aggregation. Added typed database inputs and records plus upsert, atomic page replacement, lookup, ordered-page listing, and status-count APIs. Verified insert, replacement idempotency, aggregate counts, and node-delete cascades against PostgreSQL 17.

**New files:**

- `crates/db/migrations/0013_document_text.down.sql`
- `crates/db/migrations/0013_document_text.up.sql`
- `crates/db/tests/document_text.rs`

**Modified files:**

- `crates/db/src/lib.rs`
- `docs/ocr.md`

---

### Story 13.3 — Embedded PDF Text Detection & Extraction

As a system, I want PDFs that already contain a text layer to have that text stored directly so that OCR is skipped for files that do not need it. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] A detection step reads a PDF's embedded text before any OCR work is scheduled for it.
- [x] The rule from the answers is implemented literally: **any** usable embedded text means the text is stored and OCR is skipped for that file.
- [x] "Usable" is defined by a documented threshold (for example, a minimum count of non-whitespace characters across the document) so that a PDF carrying only a stray ligature or an empty text layer is not mistaken for a text PDF; the threshold lives in configuration, not a literal.
- [x] Extracted embedded text is written to `document_text` with `source = 'embedded'`, `status = 'completed'`, and per-page rows in `document_text_pages`.
- [x] Files skipped this way record `status = 'skipped'` on any queued OCR work rather than being silently dropped, so the OCR page can report them.
- [x] Extraction reuses the existing Tika path where practical — `crates/media/src/tika.rs` already streams documents to the Tika container, and `/tika` returns text where `/meta` returns metadata.
- [x] Per-page boundaries are preserved; if the chosen endpoint cannot report pages, the story documents the fallback (single page 1 row) and records a warning on the record.
- [x] Tests: a text-layer PDF stores text and skips OCR; a scanned image-only PDF is routed to OCR; an empty-text-layer PDF is routed to OCR.

**Implementation report:** Added a bounded Tika `/tika` text adapter that always sends `X-Tika-PDFOcrStrategy: no_ocr`, classifies embedded text using the global `OCR_EMBEDDED_TEXT_MIN_CHARS` threshold (default 20), and never invokes Tesseract during detection. The OCR queue handler stores usable embedded content with `source = 'embedded'` and `status = 'completed'`, writes the endpoint's single-page fallback with an explicit page-boundary warning, and marks the leased OCR job `skipped`. Adapter tests distinguish usable, scanned, and stray/empty text responses; a PostgreSQL worker integration test verifies persistence and the skipped queue outcome.

**New files:**

- `crates/worker/src/ocr.rs`
- `crates/worker/tests/ocr_embedded.rs`

**Modified files:**

- `.env.example`
- `crates/media/src/lib.rs`
- `crates/media/src/tika.rs`
- `crates/worker/src/lib.rs`
- `crates/worker/src/main.rs`
- `docker-compose.prod.yml`
- `docs/ocr.md`

---

### Story 13.4 — OCR Job Type & Queue Integration

As a developer, I want `ocr` to be a first-class job type so that OCR work is durable, leased, and retried like every other background job. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] A migration adds `ALTER TYPE job_type ADD VALUE IF NOT EXISTS 'ocr'`, following the pattern established by `crates/db/migrations/0011_import_scan_job_type.up.sql`.
- [x] The existing partial index `jobs_active_type_target_unique` already covers per-node uniqueness for every non-`import_scan` type, so one active OCR job per node is guaranteed without a new index; the story verifies this rather than adding a redundant one.
- [x] `JobType::Ocr` is added to the enum in `crates/db/src` and to the dispatch match in `crates/worker/src/lib.rs:163`.
- [x] `ocr` is added to the claim-order list in `crates/worker/src/lib.rs:317` **after** `MetadataExtraction` and `PreviewGeneration`, so that interactive-facing work is not starved by long OCR runs.
- [x] OCR jobs use a longer `max_attempts` and lease TTL than metadata jobs, or the story documents why the existing defaults are adequate; lease renewal already exists via `handle_with_lease_renewal`.
- [x] `GET /api/jobs` and `GET /api/jobs/:id` return OCR jobs without change to their response shape.
- [x] Test: an enqueued OCR job is claimed, leased, renewed past the base TTL, and completed.
- [x] Test: enqueueing a second OCR job for a node with one already pending is rejected by the unique index.

**Implementation report:** Added the `ocr` job type and a durable `skipped` outcome through migration 0014, typed both in the database layer, and routed OCR work through `WorkerHandler`. OCR is claimed after metadata and preview work, receives five attempts instead of three, and uses three times the base lease TTL while retaining periodic renewal. The existing partial active-job index supplies OCR uniqueness without another index. PostgreSQL tests verify duplicate suppression and completion after renewal beyond the initial TTL; API integration coverage verifies that both existing jobs endpoints return OCR jobs using their unchanged JSON shapes.

**New files:**

- `crates/api/tests/jobs_api.rs`
- `crates/db/migrations/0014_ocr_job_type.down.sql`
- `crates/db/migrations/0014_ocr_job_type.up.sql`

**Modified files:**

- `crates/db/src/lib.rs`
- `crates/db/tests/jobs.rs`
- `crates/worker/Cargo.toml`
- `crates/worker/src/lib.rs`
- `docs/ocr.md`

---

## Epic 14 — OCR Engine & Worker Pipeline

**Goal:** Every supported image and image-only PDF is automatically OCR'd by a bounded, isolated Tesseract process, with page text, language, and confidence persisted.

**Sprint Capacity Estimate:** 1–2 sprints

---

### Story 14.1 — Tesseract Adapter

As a developer, I want a Tesseract adapter alongside the existing extractor adapters so that OCR follows the same shape as ExifTool, ffprobe, and Tika. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `crates/media/src/ocr.rs` is added next to the existing `exif.rs`, `ffprobe.rs`, `tika.rs`, and `office.rs` adapters and is exported from `crates/media/src/lib.rs`.
- [x] The adapter invokes Tesseract through the existing `crates/media/src/process.rs` helper rather than spawning processes directly.
- [x] TSV output is requested so that per-word confidence is available; page-level confidence is the aggregate of word confidences, and the aggregation method (mean weighted by character count, or plain mean) is documented in the ADR.
- [x] The adapter returns a typed result carrying per-page text, per-page confidence, detected page dimensions, the language used, and any warnings — not a raw string.
- [x] The OCR language is read from one global configuration value (for example `STRIFE_OCR_LANGUAGE`, defaulting to `eng`), consistent with the "global, not per-file" decision.
- [x] The adapter reports the Tesseract version so `document_text.engine_version` is populated and version-based reprocessing is possible later.
- [x] A missing or non-executable Tesseract binary produces a clear, actionable error at worker startup rather than a per-job failure.
- [x] `tesseract-ocr` and the `eng` language pack are installed in `deploy/docker/backend.Dockerfile`, and the worker image is verified to contain them.
- [x] Unit tests run the adapter against a small committed fixture image with known text and assert the text, a plausible confidence, and the reported version.

**Implementation report:** Added a typed Tesseract TSV adapter using the shared bounded-process runner. It reconstructs lines and pages, calculates character-weighted confidence, carries dimensions/language/warnings/version, verifies the configured binary during worker startup, and reports Linux peak RSS. The production worker image was built and verified with Tesseract 5.5.0 and the `eng` language data installed; adapter tests use a deterministic executable fixture and known OCR payload.

**New files:**

- `crates/media/src/ocr.rs`

**Modified files:**

- `.env.example`
- `crates/media/src/lib.rs`
- `crates/media/src/process.rs`
- `crates/worker/src/lib.rs`
- `crates/worker/src/main.rs`
- `deploy/docker/backend.Dockerfile`
- `docker-compose.prod.yml`
- `docs/ocr.md`

---

### Story 14.2 — Input Normalization & Format Routing

As a system, I want every supported input converted into something Tesseract can read so that PDFs and raw images are OCR'd, not skipped. **Estimated: 8 points.**

**Acceptance Criteria:**

- [x] MIME-based routing is added to `crates/worker/src/metadata.rs`, which already routes by detected MIME rather than file extension.
- [x] The full input list from the answers is supported: PDF, JPEG, PNG, TIFF, WebP, and raw camera images.
- [x] Image-only PDFs are rasterized to per-page images before OCR, and the resulting page numbers match the source PDF's page order.
- [x] Raw images are decoded through the same path already used for RAW previews so a second decoder is not introduced.
- [x] Multi-page TIFFs produce one `document_text_pages` row per page.
- [x] Rasterization resolution is configurable with a documented default (150–300 DPI is the usual accuracy/cost tradeoff) rather than hard-coded.
- [x] Formats outside the supported list record `status = 'unsupported'` — distinct from `'failed'` — so the OCR page can distinguish "cannot" from "went wrong".
- [x] A scanned PDF is OCR'd exactly once. Per the decision in Story 13.1, Tika's implicit Tesseract invocation during `metadata_extraction` is either suppressed (for example by an OCR-strategy request header or parser configuration) or its output is captured; a test asserts that a scanned PDF does not consume Tesseract CPU in both the metadata job and the OCR job.
- [x] Every intermediate rasterized page is written under the managed temporary path and removed on both success and failure, including panics.
- [x] `docs/supported-formats.md` gains an OCR column stating which formats are OCR inputs.
- [x] Tests cover one fixture per supported input family, plus an unsupported format asserting `'unsupported'`.

**Implementation report:** Added MIME-driven OCR candidacy and normalization for PDF, JPEG, PNG, TIFF, WebP, and the existing LibRaw camera-raw family. Poppler emits ordered PDF pages, ImageMagick normalizes raster frames, TIFF pages remain distinct, and all intermediates live in a drop-managed temporary directory. Tests exercise raster families, multi-page TIFF, unsupported MIME routing, and a two-page image-only PDF through the worker; metadata-only Tika calls explicitly disable implicit OCR.

**Modified files:**

- `crates/media/src/ocr.rs`
- `crates/media/src/tika.rs`
- `crates/worker/src/metadata.rs`
- `crates/worker/src/ocr.rs`
- `crates/worker/tests/ocr_embedded.rs`
- `docs/supported-formats.md`
- `docs/tika.md`
- `docs/ocr.md`

---

### Story 14.3 — OCR Resource Limits & Isolation

As an operator, I want OCR bounded in pages, pixels, time, memory, and output size so that one pathological file cannot exhaust the host. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Configurable limits exist for maximum page count, maximum pixels per page, wall-clock timeout per file, memory ceiling, and maximum stored text bytes per file, each with a documented default.
- [x] Defaults are chosen as sensible starting values and explicitly marked as provisional pending profiling, matching the answer in `deferred.md`; the ADR records that they are expected to change.
- [x] Exceeding any limit fails the file with `status = 'failed'` and a specific, human-readable warning naming the limit hit — not a generic error.
- [x] A file that exceeds a limit does not consume its full retry budget, since retrying is guaranteed to hit the same limit; such failures are marked terminal.
- [x] The Tesseract process is subject to the same isolation posture as the other extractors, and any gap is recorded in `docs/known-limitations.md` rather than left implicit.
- [x] Peak memory and duration per file are logged so the profiling follow-up has data to work from; `duration_ms` is persisted on `document_text`.
- [x] Tests: a synthetic oversized input trips the page limit and the pixel limit; a stalling process trips the timeout and is killed rather than leaking.

**Implementation report:** Added provisional configurable ceilings of 100 pages, 40 million pixels per page, 600 seconds per file, 512 MiB for ImageMagick normalization, and 16 MiB of stored text. Deterministic limit failures persist specific warnings and terminate without exhausting retries. The shared process runner kills timed-out children and samples Linux RSS, while the worker persists duration and logs duration, text bytes, and peak Tesseract memory. The documented isolation gap is that Tesseract still relies on the worker container's memory ceiling.

**Modified files:**

- `.env.example`
- `crates/media/src/ocr.rs`
- `crates/media/src/process.rs`
- `crates/worker/src/lib.rs`
- `crates/worker/src/ocr.rs`
- `docker-compose.prod.yml`
- `docs/decisions/0008-ocr-and-document-text.md`
- `docs/known-limitations.md`
- `docs/ocr.md`

---

### Story 14.4 — OCR Job Handler

As a system, I want an OCR job handler that persists page text, language, and confidence so that results survive restarts and are queryable. **Estimated: 8 points.**

**Acceptance Criteria:**

- [x] A handler in `crates/worker/src` mirrors the structure of the existing metadata handler: copy the managed original to a temporary path, detect MIME from bytes, route, extract, persist, clean up.
- [x] The embedded-PDF check from Story 13.3 runs first; if usable text exists, the handler stores it as `source = 'embedded'` and completes without invoking Tesseract.
- [x] Successful OCR writes one `document_text` row and one `document_text_pages` row per page in a single transaction, so a crash mid-write cannot leave partial page sets.
- [x] Re-running OCR for a node fully replaces prior pages rather than appending, using `replace_document_text_pages`.
- [x] Failures record `status = 'failed'` with the underlying cause in `warnings`, and the job's `last_error` is populated; the underlying error is logged, not discarded — the `map_err(|_| ...)` pattern called out in Story 8.2 of `scrum.md` must not be repeated here.
- [x] The handler is idempotent: a job retried after a lease expiry produces the same stored result and no duplicate rows.
- [x] Trashed and permanently deleted nodes do not have OCR run against them, and in-flight OCR for a node deleted mid-job fails cleanly rather than writing orphan rows.
- [x] Tests: image fixture → completed with pages and confidence; scanned PDF → completed with page count matching the source; text PDF → `'embedded'` with Tesseract never invoked; corrupt file → `'failed'` with a populated warning.

**Implementation report:** Added the durable OCR handler from managed-original copy through byte-based detection, embedded-text precedence, normalization, extraction, atomic document/page replacement, event emission, and cleanup. Empty terminal states also replace the page set so failed reprocessing cannot expose stale text. Worker integration tests cover embedded PDFs, idempotent image reruns, ordered two-page scanned PDFs, corrupt input diagnostics, confidence, duration, and job outcomes; lifecycle checks skip trashed targets and cascade ownership prevents orphan text after deletion.

**Modified files:**

- `crates/db/src/lib.rs`
- `crates/worker/src/lib.rs`
- `crates/worker/src/ocr.rs`
- `crates/worker/tests/ocr_embedded.rs`
- `docs/ocr.md`

---

### Story 14.5 — Automatic OCR Enqueue on Finalization

As a user, I want OCR to start automatically for every supported file so that no manual step is required. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `finalize_upload` and `finalize_import` enqueue an `ocr` job in the same database transaction that publishes the file, exactly as they already do for `metadata_extraction` — so an OCR job cannot be lost after a successful finalization.
- [x] Enqueueing is unconditional for candidate MIME types; the decision to skip is made by the handler, not the enqueue site, so that files whose MIME is only known after byte inspection are still considered.
- [x] Watched-folder imports and direct uploads produce identical OCR behavior for the same file.
- [x] Enqueue failures do not roll back the finalization itself; the file remains published and the missing job is recoverable by the reprocessing path in Story 14.6.
- [x] `docs/tika.md`'s end-to-end flow diagram is extended to show the OCR branch alongside the existing ExifTool, ffprobe, and Tika branches.
- [x] Test: uploading a scanned PDF results in a pending OCR job without any user action.
- [x] Test: importing the same file through `/mnt/ext/watch` produces the same job.

**Implementation report:** Extended both upload and watched-folder finalization transactions with best-effort OCR enqueueing. Enqueueing is content-agnostic and idempotent, uses five attempts and low priority, and is isolated behind a savepoint so publishing still succeeds if queue insertion fails. API, database, and importer integration tests assert that both ingestion paths publish the same pending OCR work automatically.

> [!NOTE]
> Automatic enqueueing applies only when a file is finalized after this behavior is deployed. It must not be extended into a startup or migration scan of the existing library; historical OCR is governed by Story 16.6 and [`backfill.md`](backfill.md).

**Modified files:**

- `crates/api/src/uploads.rs`
- `crates/api/tests/uploads_api.rs`
- `crates/db/src/lib.rs`
- `crates/importer/src/lib.rs`
- `crates/importer/tests/e2e_import.rs`
- `crates/importer/tests/import_pipeline.rs`
- `docs/tika.md`
- `docs/ocr.md`

---

### Story 14.6 — Manual OCR Reprocessing

As a user, I want to re-trigger OCR so that files can be reprocessed after an engine, language, or limit change. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `POST /api/admin/reprocess` accepts `ocr` in its extractor whitelist, which today accepts only `exiftool`, `ffprobe`, and `tika` in `crates/api/src/admin.rs`.
- [x] Reprocessing can be scoped to a single node, to all failed files, or to all files whose `engine_version` differs from the current Tesseract version — the version case is what makes engine upgrades actionable.
- [x] The endpoint returns the number of jobs enqueued, matching the existing `ReprocessResponse` shape.
- [x] Re-enqueueing a node that already has a pending OCR job is a no-op rather than an error, so a double-click cannot fail the request.
- [x] Reprocessing is exposed in the UI from the OCR page built in Story 16.4, not only through the raw API.
- [x] Bulk reprocessing is bounded per request so a whole-library re-OCR cannot enqueue unboundedly in one transaction.
- [x] Tests: single-node re-trigger; version-mismatch bulk enqueue; no-op on an already-pending node.

**Implementation report:** Added OCR scopes to the existing reprocess endpoint for one node, failed records, and engine-version mismatches, retaining the existing enqueue-count response. Requests are clamped to 100 candidates, active jobs are excluded before bulk limits are applied, and duplicate node requests are no-ops. The OCR page exposes per-file retry and confirmed failed-file bulk reprocessing; PostgreSQL and API tests cover every scope and duplicate suppression.

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/views/OcrStatusView.tsx`
- `crates/api/src/admin.rs`
- `crates/api/tests/ocr_api.rs`
- `crates/db/src/lib.rs`
- `crates/db/tests/ocr_operations.rs`
- `docs/ocr.md`

---

## Epic 15 — Document Text Search

**Goal:** Stored OCR and embedded text becomes searchable across the whole drive, which is the stated reason for keeping text in PostgreSQL.

**Sprint Capacity Estimate:** 1 sprint

> [!NOTE]
> The broader **Search and Organization** questions in [`deferred.md`](../deferred.md) — filename matching strategy, filters, trash inclusion, saved searches, semantic search — remain open. This epic deliberately covers only document-text search so that OCR delivers its stated value without pre-empting those decisions.

---

### Story 15.1 — Full-Text Search Index

As a developer, I want a PostgreSQL full-text index over stored text so that content search is fast enough to be interactive. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] A migration adds a generated `tsvector` column over `document_text_pages.content` with a GIN index.
- [x] The text search configuration matches the English-only decision, and the ADR records what must change when a second language is added.
- [x] Indexing is per page, not per document, so that results can report the page a match occurred on.
- [x] A ranking function orders results by relevance rather than by insertion order.
- [x] The index is populated for existing rows by the migration, not only for subsequent inserts.
- [x] A documented benchmark records query latency against a seeded corpus so the "dedicated search service" question in `deferred.md` can later be decided on measurement rather than intuition.
- [x] Tests: a term present in one page of a multi-page document returns that page and not its siblings.

**Implementation report:** Added a stored English `tsvector` per text page and a GIN index that backfills existing rows through PostgreSQL's generated-column migration. Search uses `websearch_to_tsquery` and `ts_rank_cd`, preserving page identity and relevance ordering. A rollback-only 10,000-page benchmark used the GIN index with 0.406 ms planning and 1.223 ms execution on the development ARM64 host; the ADR records the migration work required for another language.

**New files:**

- `crates/db/migrations/0016_document_text_search.down.sql`
- `crates/db/migrations/0016_document_text_search.up.sql`
- `docs/benchmarks/ocr-search.md`

**Modified files:**

- `crates/db/src/lib.rs`
- `crates/db/tests/ocr_operations.rs`
- `docs/decisions/0008-ocr-and-document-text.md`
- `docs/ocr.md`

---

### Story 15.2 — Text Search API

As a user, I want an API that searches document text so that the frontend can surface content matches. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `GET /api/search` accepts a query string and returns matching nodes with, per match, the page number, a contextual snippet, and a relevance score.
- [x] Snippets are generated by PostgreSQL's headline function so that matched terms are highlighted consistently.
- [x] Trashed nodes are excluded by default; including them requires an explicit parameter.
- [x] Results are paginated using the same cursor convention as the existing folder-listing endpoints, not an offset scheme.
- [x] An empty or whitespace-only query returns a `400` with the unified error body rather than scanning the corpus.
- [x] The endpoint returns the shared error shape and logs internal causes, consistent with Epic 8 of `scrum.md`.
- [x] Integration tests cover a hit, a miss, a multi-page document, a trashed-node exclusion, and pagination across a result set larger than one page.

**Implementation report:** Added `GET /api/search` with validated nonblank queries, PostgreSQL-generated highlighted snippets, relevance scores, page numbers, active-node filtering, explicit trash inclusion, and UUID cursor pagination. Responses also report the indexed-document count so the UI can distinguish an empty index from a miss. API integration coverage exercises hits, misses, page specificity, trash behavior, pagination, and the shared 400 error response.

**New files:**

- `crates/api/src/search.rs`

**Modified files:**

- `crates/api/src/lib.rs`
- `crates/api/tests/ocr_api.rs`
- `docs/ocr.md`

---

### Story 15.3 — Search UI with Snippets

As a user, I want to search file contents from the interface so that I can find documents by what is inside them. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] The search input in the topbar queries `/api/search` and renders results in the existing file-table vocabulary rather than a separate visual language.
- [x] Each result shows the file name, the matching page number, and a snippet with the matched terms emphasized.
- [x] Selecting a result opens the file, and where the preview supports paging, it opens at the matching page.
- [x] Query input is debounced so that typing does not issue a request per keystroke.
- [x] Loading, empty-result, and error states are all handled; the empty state distinguishes "no matches" from "no text extracted yet".
- [x] Search is keyboard-navigable and the results list is announced to assistive technology.
- [x] `eslint --max-warnings 0` and `prettier --check` pass.

**Implementation report:** Added a 300 ms debounced content-search control to workspace pages using the existing file vocabulary. Results show name, page, safely parsed PostgreSQL highlights, loading/error/no-match/no-index states, and open the native preview at the matching page. The combobox status is announced, results use keyboard-focusable options, and the production TypeScript/Vite build, ESLint with zero warnings, and Prettier check pass.

**New files:**

- `apps/web/src/components/ContentSearch.css`
- `apps/web/src/components/ContentSearch.tsx`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/views/WorkspaceView.tsx`
- `docs/ocr.md`

---

## Epic 16 — OCR Status & Text UI

**Goal:** OCR is observable and its output readable — a sidebar entry leads to a page with counts, status, and a live console, and extracted text is visible and copyable per file.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 16.1 — OCR Status API

As a user, I want an endpoint reporting OCR progress so that the OCR page can show how much work remains. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `GET /api/ocr/status` returns counts by state: pending, running, completed, failed, skipped, and unsupported.
- [x] The response includes the count of documents still to be processed — the "how many documents will be OCR'ed" figure the answers call for — derived from pending and leased `ocr` jobs.
- [x] The response includes the current engine name and version so a version mismatch across the library is visible.
- [x] Counts come from indexed aggregate queries, not from loading rows into the application.
- [x] The endpoint returns the shared error body and has an integration test, satisfying the coverage rule in Story 8.5 of `scrum.md`.
- [x] Tests cover an empty library, a mixed-state library, and a library where every file is complete.

**Implementation report:** Added indexed aggregate OCR status queries and `GET /api/ocr/status`, returning pending, running, completed, failed, skipped, unsupported, remaining, and the current worker-reported engine/language. An isolated PostgreSQL test moves from an empty database through exact mixed counts to an all-complete library, while API coverage verifies the public response and shared error handling.

**New files:**

- `crates/db/migrations/0015_ocr_operations.down.sql`
- `crates/db/migrations/0015_ocr_operations.up.sql`
- `crates/db/tests/ocr_status_states.rs`

**Modified files:**

- `crates/api/src/ocr.rs`
- `crates/api/tests/ocr_api.rs`
- `crates/db/src/lib.rs`
- `crates/worker/src/main.rs`
- `docs/ocr.md`

---

### Story 16.2 — OCR Event Stream

As a user, I want a live event stream of OCR activity so that the OCR page updates without polling. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `GET /api/ocr/events` streams server-sent events, following the established `stream_import_events` implementation in `crates/api/src/imports.rs:231`.
- [x] An `entry` event is emitted per file as it starts, completes, fails, or is skipped, carrying node id, name, state, page count, and mean confidence.
- [x] A `status` event carries the aggregate counts from Story 16.1 so the page's summary stays current without a second request.
- [x] Keep-alive matches the import stream so proxies do not sever an idle connection.
- [x] The stream resumes from a cursor after reconnection rather than replaying the full history, mirroring `list_import_entry_events_after`.
- [x] Disconnecting clients release their database resources; a test asserts no connection leak across repeated connect/disconnect cycles.

**Implementation report:** Added durable OCR activity events and an SSE endpoint modeled on import streaming. Entry events carry file/state/page/confidence/warning data, status events refresh aggregates, cursors honor `Last-Event-ID`, new clients begin at the current maximum event, and 15-second keep-alives preserve idle connections. API integration tests reconnect and disconnect repeatedly, then confirm the pool remains usable.

**New files:**

- `crates/api/src/ocr.rs`

**Modified files:**

- `crates/api/src/lib.rs`
- `crates/api/tests/ocr_api.rs`
- `crates/db/src/lib.rs`
- `crates/worker/src/ocr.rs`
- `docs/ocr.md`

---

### Story 16.3 — Sidebar OCR Navigation

As a user, I want an OCR entry in the sidebar so that the OCR page is reachable. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] An `{ href: '/ocr', label: 'OCR', icon: … }` entry is added to the `navigation` array in `apps/web/src/components/Sidebar.tsx:8`, placed after `Imports` so ingestion-related entries stay grouped.
- [x] A `/ocr` route is registered in `apps/web/src/index.tsx`.
- [x] The entry shows a count of pending documents, following the pattern the `Errors` entry already uses for its failed-entry count.
- [x] The count is omitted rather than shown as `0` when no work is pending, so the sidebar stays quiet at rest.
- [x] The active state uses the existing `is-active` treatment; no new nav styling is introduced.
- [x] The entry renders in static preview mode (`VITE_STATIC_PREVIEW=true`) without a backend.

**Implementation report:** Added OCR immediately after Imports in the shared sidebar and registered `/ocr`. The sidebar loads the remaining count, omits a zero badge, reuses the established active state, and supplies deterministic static-preview status without contacting the backend.

**Modified files:**

- `apps/web/src/components/Sidebar.tsx`
- `apps/web/src/index.tsx`
- `docs/ocr.md`

---

### Story 16.4 — OCR Status Page & Live Console

As a user, I want an OCR page showing counts, status, and live progress so that I can see what OCR is doing. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `apps/web/src/views/OcrStatusView.tsx` follows the structure of `ImportStatusView.tsx`, which already implements the counts-plus-SSE-console pattern this page is specified to mirror.
- [x] A counts panel shows pending, running, completed, failed, skipped, and unsupported, using the same `Count` tile treatment as the import page.
- [x] A console with `role="log"` and `aria-live="polite"` streams per-file OCR activity from `/api/ocr/events`.
- [x] The console shows a connection indicator with `connecting`, `live`, and `reconnecting` states, matching the import page's `streamStatus` handling.
- [x] Failed files show their warning text inline and offer a per-file re-trigger wired to Story 14.6.
- [x] A bulk "Reprocess failed" action is available and confirms before enqueueing.
- [x] The console is bounded to a fixed number of retained entries so a long run cannot grow the DOM without limit; the import console's 200-entry cap is the precedent.
- [x] The page renders with sample data under `VITE_STATIC_PREVIEW=true`.
- [x] `eslint --max-warnings 0` and `prettier --check` pass.

**Implementation report:** Added an OCR status view matching the import counts-and-console structure. It consumes status entirely through SSE after initial load, prepends newest events, retains at most 200 rows, exposes connection state and inline warnings, and supports per-file and confirmed bulk retry. Static preview sample data, the production frontend build, ESLint with zero warnings, and Prettier all pass.

**New files:**

- `apps/web/src/views/OcrStatusView.css`
- `apps/web/src/views/OcrStatusView.tsx`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/index.tsx`
- `docs/ocr.md`

---

### Story 16.5 — Extracted Text Panel

As a user, I want to read and copy a file's extracted text so that OCR output is directly useful. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `GET /api/files/:id/text` returns the stored text for a node with per-page content, confidence, language, engine version, and warnings.
- [x] The file details panel gains a text section showing the extracted text with visible page boundaries rather than one undifferentiated block.
- [x] Text is selectable and copyable, with a copy control for the whole document and per page.
- [x] Page-level confidence is displayed, and pages below a documented threshold are visually flagged as low-confidence.
- [x] Extraction warnings are shown alongside the text, not hidden behind a control.
- [x] The panel distinguishes its states: not yet processed, in progress, completed, failed, skipped because embedded text was used, and unsupported.
- [x] Text is read-only with no edit affordance, per the recorded decision; the panel does not imply correction is possible.
- [x] Long documents virtualize or paginate so that a several-hundred-page file does not stall the browser.
- [x] `eslint --max-warnings 0` and `prettier --check` pass.

**Implementation report:** Added paginated `GET /api/files/:id/text` and a read-only details-panel text section. It renders explicit extraction states, language/engine/warnings, visible page boundaries, selectable content, whole-document and per-page copy controls, and a documented low-confidence treatment below 70%. API pagination is capped at 50 pages and the panel incrementally loads long documents; frontend formatting, lint, and production build pass.

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/components/FileDetailsPanel.css`
- `apps/web/src/components/FileDetailsPanel.tsx`
- `crates/api/src/files.rs`
- `crates/api/tests/ocr_api.rs`
- `docs/ocr.md`

---

### Story 16.6 — Historical OCR Backfill Campaign

As an operator, I want historical OCR admitted through an explicit bounded campaign so that deploying OCR and email support cannot saturate Orion with the existing library. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] OCR deployment, migrations, API startup, worker startup, and recovery do not automatically enqueue historical nodes; files finalized after deployment continue to receive foreground OCR jobs.
- [x] OCR adopts the shared `backfill_campaigns`, job origin/campaign fields, durable cursor, scheduler, priority policy, and renewable resource leases specified by [`backfill.md`](backfill.md).
- [x] A read-only preflight reports OCR candidates by MIME family, file/page/size percentiles where discoverable without OCR, current text state, projected work, and estimated storage without enqueueing jobs.
- [x] A historical OCR campaign starts paused and records candidate snapshot criteria, batch size, maximum queued/running work, resource class, cursor, counts, timestamps, and initiating version.
- [x] Initial Orion defaults are 100 candidates per refill, at most 500 queued OCR jobs, one running OCR backfill job, and one shared `HEAVY_CPU` permit across OCR, email, and attachment backfills.
- [x] Foreground jobs outrank repair work, which outranks historical OCR; a fairness budget allows slow backfill progress without hiding new uploads, imports, metadata, previews, or deletions.
- [x] Pausing stops refills while leased work finishes; resuming continues from the durable cursor; cancelling prevents new claims without deleting completed OCR text.
- [x] The OCR page distinguishes foreground activity from campaigns and exposes candidate count, state, progress, throughput, ETA, limits, start/pause/resume/cancel controls, and canary results.
- [x] The project owner explicitly records ordinary historical OCR before email body parsing for the first Orion rollout; email attachment OCR remains after both and uses the same OCR/shared-heavy permit.
- [x] Tests cover inert deployment/startup, foreground processing while paused, canary limits, low-water refill, cross-pipeline mutual exclusion, priority/fairness, pause/resume/cancel, restart recovery, and multi-worker resource-lease enforcement.

**Implementation progress:** Registered the OCR adapter on the shared coordinator and implemented historical candidate selection end to end. Candidate selection, enqueue, and `(created_at, id)` cursor advance share one transaction, so an interrupted refill can neither skip nor repeat a file. Candidates are active finalized files whose extracted `detected_mime` is an OCR input and whose document text is absent or from a different engine version; files still awaiting metadata are counted separately rather than guessed at from their filename. Added a read-only preflight endpoint reporting candidates and byte percentiles per MIME family, and OCR page controls for preflight, paused campaign creation from a reviewed report, resume, pause, and cancel. Two safety guards are enforced in code: an unprepared campaign has no frozen snapshot and refuses to enumerate, and a worker with no verified Tesseract refuses to refill rather than treating every file as a version mismatch and enqueueing the whole library.

The OCR page now adds live pending/running/remaining counts, recent throughput, an estimated completion time, and recorded canary results, refreshed every 15 seconds. Initial campaign creation uses an exact 100-item cumulative cap; the shared coordinator stops enqueueing at that boundary and automatically returns the campaign to paused after its queue drains. A guarded stage control advances that same campaign to 1,000, 10,000, and then full only in order, only with no active campaign jobs, and only after the current stage has a recorded approved result.

Test coverage includes restart and foreground isolation: a new coordinator resumes from the durable cursor without duplicates, paused history does not prevent a foreground OCR claim, and an exact 100-of-105 canary auto-pauses after drain. API coverage proves an unapproved result, a skipped stage, or an active job blocks advancement, then walks the approved 100 → 1,000 → 10,000 → full sequence. On 2026-08-03 the project owner explicitly selected ordinary OCR before email bodies for the first Orion rollout, with every attachment stage still last; [`backfill.md`](backfill.md) records that operational sequence.

**New files:**

- `crates/db/tests/ocr_backfill_candidates.rs`
- `crates/worker/tests/ocr_backfill.rs`

**Modified files:**

- `Cargo.lock`
- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/views/OcrStatusView.css`
- `apps/web/src/views/OcrStatusView.tsx`
- `crates/api/Cargo.toml`
- `crates/api/src/ocr.rs`
- `crates/db/Cargo.toml`
- `crates/db/src/lib.rs`
- `crates/media/src/lib.rs`
- `crates/media/src/ocr.rs`
- `crates/worker/src/backfill.rs`
- `crates/worker/src/lib.rs`
- `docs/ocr.md`

---

## Summary

| Epic                                         | Milestone | Stories | Estimated Points |
| -------------------------------------------- | --------- | ------- | ---------------- |
| 13 — OCR Decisions & Text Storage Foundation | M13       | 4       | 15               |
| 14 — OCR Engine & Worker Pipeline            | M14       | 6       | 32               |
| 15 — Document Text Search                    | M15       | 3       | 15               |
| 16 — OCR Status & Text UI                    | M16       | 6       | 23               |
| **OCR Total**                                |           | **19**  | **85**           |

> [!TIP]
> At the ~30 points/sprint velocity assumed in [`scrum.md`](../scrum.md), this is roughly **3 sprints** of work on top of the 105 stories already planned there. Story 16.6 is the production backfill amendment required by the later email plan.

> [!IMPORTANT]
> Suggested order: **13.1 and 13.2 first** — the schema is the contract every other story writes against, and changing it after the worker and UI depend on it is the expensive mistake here. **13.3 before Epic 14**, because embedded-text detection determines how much OCR work actually exists and may substantially reduce it. Epic 15 can proceed in parallel with Epic 16 once text is landing in the database.

> Feature deployment and historical processing are separate. Ship OCR support with historical campaigns paused, process new files as foreground work, then follow [`backfill.md`](backfill.md) for preflight, email-first canaries, and the later historical OCR campaign.

> [!WARNING]
> Two questions from `deferred.md` are answered only provisionally and are carried as follow-ups by Story 13.1: the concrete page, pixel, time, memory, and output-size limits ("this may need to be profiled, use sensible defaults for now"), and the reprocessing policy when OCR models or tool versions change, which Story 14.6 makes possible but does not make automatic. Neither should be treated as settled.
