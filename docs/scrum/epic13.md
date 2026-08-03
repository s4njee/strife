# Epic 13 — OCR Decisions & Text Storage Foundation


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
