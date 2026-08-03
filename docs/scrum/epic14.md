# Epic 14 — OCR Engine & Worker Pipeline


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
> Automatic enqueueing applies only when a file is finalized after this behavior is deployed. It must not be extended into a startup or migration scan of the existing library; historical OCR is governed by Story 16.6 and [`backfill.md`](../backfill.md).

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
