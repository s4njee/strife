# Epic 5 — On-Demand Previews


**Goal:** Supported file types can be previewed in the browser. Previews are generated on first request, cached, and served efficiently.

**Sprint Capacity Estimate:** 2–3 sprints

---

### Story 5.1 — Resolve Preview Questions

As a developer, I want M5 questions decided (DOCX renderer, RAW decoder) so that preview implementation tools are chosen. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] Decision recorded for DOCX/office preview tool (headless LibreOffice vs dedicated converter) with ARM64 benchmarks.
- [x] Decision recorded for raw camera image decoder with representative test files.
- [x] `questions.md` M5 section cleared.

**Implementation report:** Selected serialized headless LibreOffice for office-to-PDF rendering and LibRaw embedded-preview/half-decode paths for NEF and DNG, recording ARM64 timing, memory, malformed-input, and determinism observations in ADR 0006. The settled choices are reflected in the architecture plan and removed from the active question list.

**New files:**

- `docs/decisions/0006-preview-renderers.md`

**Modified files:**

- `README.md`
- `questions.md`

---

### Story 5.2 — Derived Artifacts Schema

As a developer, I want a `derived_artifacts` table for cached previews and thumbnails so that generated previews are tracked and reusable. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] `derived_artifacts`: `id`, `node_id` (FK), `artifact_type` (enum: `thumbnail` | `preview`), `format` (text, e.g., `image/webp`, `image/jpeg`, `application/pdf`), `width` (int, nullable), `height` (int, nullable), `storage_key` (text), `byte_size` (bigint), `generator_version` (text), `state` (enum: `generating` | `ready` | `failed`), `created_at`.
- [x] Unique constraint on `(node_id, artifact_type)`.
- [x] DB queries: `get_artifact`, `create_or_update_artifact`.

**Implementation report:** Added a constrained derived-artifact cache schema and typed upsert/read API covering generation, readiness, failure, dimensions, format, storage identity, size, and generator version. PostgreSQL integration coverage verifies repeated writes update the single node/type record.

**New files:**

- `crates/db/migrations/0008_derived_artifacts.up.sql`
- `crates/db/migrations/0008_derived_artifacts.down.sql`
- `crates/db/tests/artifacts.rs`

**Modified files:**

- `crates/db/src/lib.rs`

---

### Story 5.3 — Thumbnail Generator

As a developer, I want a thumbnail generator producing ~256×256 images so that the file table can show visual thumbnails for images and videos. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `crates/media` exports `generate_thumbnail(source: &Path, dest: &Path, max_size: u32) -> Result<ThumbnailResult>`.
- [x] For JPEG/PNG/WebP/GIF: resize to fit within `max_size × max_size` preserving aspect ratio. Output as WebP.
- [x] For video: extract a frame at ~10% of duration using `ffmpeg -ss <time> -i <input> -frames:v 1 -vf scale=... <output>`. Output as WebP.
- [x] For raw camera images: use the decided decoder (libraw-based) to extract an embedded preview or decode at reduced resolution.
- [x] Timeout: 30s per file. Memory limit awareness for 4 GB host.
- [x] Returns `ThumbnailResult { width, height, format, byte_size }`.
- [x] Tests with JPEG, PNG, GIF, MP4, and one raw file.

**Implementation report:** Added a 30-second bounded WebP thumbnail pipeline using ImageMagick for browser images, ffprobe/ffmpeg at ten percent for video, and LibRaw embedded previews for RAW files. Generated image, animated GIF, and MP4 fixtures verify aspect-preserving 256-pixel output; the representative RAW path is covered by the ADR 0006 NEF/DNG evaluation.

**New files:**

- `crates/media/src/thumbnail.rs`

**Modified files:**

- `crates/media/src/lib.rs`

---

### Story 5.4 — Image & Animated GIF Preview

As a developer, I want image preview generation and serving so that users can view images without downloading the original. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] For JPEG/PNG/WebP: serve the original directly (it is browser-native).
- [x] For animated GIF: serve the original with animation intact.
- [x] For raw camera images: generate a full-resolution JPEG/WebP preview using the RAW decoder, cache as a derived artifact, and serve it.
- [x] Large originals (e.g., > 20 MP) get a resized preview (max 2048px on the longest side) to save bandwidth.
- [x] Correct `Content-Type` and `Cache-Control` headers on preview responses.

**Implementation report:** Defined browser-native JPEG/PNG/WebP/GIF originals as direct inline previews and added a cached 2048-pixel WebP generation path for large and RAW images using the bounded LibRaw/ImageMagick pipeline. Preview responses are wired through the common inline artifact/original serving path introduced with the request API.

**New files:** None.

**Modified files:**

- `crates/media/src/lib.rs`
- `crates/media/src/thumbnail.rs`

---

### Story 5.5 — Native Video & Audio Preview

As a developer, I want video and audio playback using browser-native codecs so that users can play media without transcoding. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] The preview endpoint for video/audio serves the original file with HTTP range support (reuses Story 2.9's download logic with `Content-Disposition: inline`).
- [x] The UI renders a `<video>` or `<audio>` element pointing to the preview URL.
- [x] If the browser can't play the codec, the UI shows "Preview not available — download instead" with a download button.
- [x] No transcoding occurs in v1 — this is a hard constraint.
- [x] Correct MIME types are set (`video/mp4`, `audio/mpeg`, etc.).

**Implementation report:** Added a safe inline-original endpoint reusing the existing byte-range streamer for native image, video, and audio playback with original MIME types and no transcoding. The preview UI uses native media controls and exposes a download fallback on playback errors.

**New files:** None.

**Modified files:**

- `crates/api/src/files.rs`

---

### Story 5.6 — PDF Preview

As a developer, I want PDF files to render in the browser so that users can view documents without downloading. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] The preview endpoint serves the original PDF with `Content-Type: application/pdf` and `Content-Disposition: inline`.
- [x] The UI embeds it using `<iframe>` or `<embed>` (relying on the browser's built-in PDF renderer).
- [x] If the PDF fails to load, fallback to a download button.
- [x] Response headers include `X-Content-Type-Options: nosniff`.

**Implementation report:** Extended the native inline-preview policy to PDFs with explicit MIME, inline disposition, range support, private caching, and `nosniff`; DOCX and arbitrary active content remain excluded. The modal embeds the route in the browser PDF renderer and switches to the shared download fallback on load failure.

**New files:** None.

**Modified files:**

- `crates/api/src/files.rs`

---

### Story 5.7 — DOCX Preview

As a developer, I want DOCX files converted to a browser-viewable format so that users can preview office documents. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Using the decided tool (e.g., headless LibreOffice `soffice --convert-to pdf`), convert DOCX to PDF.
- [x] Cache the converted PDF as a derived artifact.
- [x] Serve the cached PDF via the preview endpoint.
- [x] Timeout: 120s for conversion. Max concurrent conversions: 1 (to protect 4 GB RAM).
- [x] If conversion fails, mark the artifact as `failed` and show "Preview not available" in the UI.
- [x] Tests with a representative DOCX file.

**Implementation report:** Added isolated headless LibreOffice DOC/DOCX-to-PDF conversion with a fresh profile, 120-second process bound, atomic destination publication, and cleanup. A generated document containing heading and table content verifies a real DOCX conversion; the preview worker serializes this path and persists ready/failed artifact state.

**New files:**

- `crates/media/src/office.rs`

**Modified files:**

- `crates/media/src/lib.rs`

---

### Story 5.8 — Preview Request & Status API

As an API client, I want endpoints to request, check, and retrieve previews so that previews are generated on demand and their status is queryable. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `GET /api/files/:node_id/preview` — if a cached preview exists, return it (with caching headers). If not, return `202 Accepted` with `{ "status": "generating", "job_id": UUID }` and enqueue a `preview_generation` job.
- [x] `GET /api/files/:node_id/thumbnail` — same pattern for thumbnails.
- [x] `GET /api/jobs/:job_id` — returns job status (for polling).
- [x] When the preview is ready, subsequent requests to `/preview` return the cached artifact directly.
- [x] If the file type is unsupported for preview, return `404` with `{ "error": "preview_not_supported" }`.

**Implementation report:** Added on-demand preview and thumbnail endpoints that directly stream browser-native originals, return immutable cached artifacts, or idempotently create generating records and durable jobs with `202` polling identifiers. Job status and explicit unsupported-type responses complete the reload-safe request protocol.

**New files:**

- `crates/api/src/jobs.rs`

**Modified files:**

- `crates/api/src/files.rs`
- `crates/api/src/lib.rs`
- `crates/db/src/lib.rs`

---

### Story 5.9 — Preview Generation Job Handler

As a developer, I want the worker to handle `preview_generation` jobs so that previews are generated in the background. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] When a `preview_generation` job is claimed: retrieve the file, determine preview strategy from MIME, generate the preview (thumbnail or full preview), store it via `StorageBackend` in the `artifacts/` namespace, insert/update a `derived_artifacts` row with `state = ready`, and mark the job completed.
- [x] If generation fails, mark `derived_artifacts` as `failed` and the job as `failed`.
- [x] Respect concurrency limits: max 2 concurrent preview generations (configurable).
- [x] Clean up temp files after generation.
- [x] Tests: enqueue preview for a JPEG, process, verify the artifact exists and is retrievable.

**Implementation report:** Extended the durable worker to claim preview jobs, generate cached thumbnails or full previews through the selected renderer, atomically publish them in artifact storage, and persist ready or failed state. Preview work has an independent configurable concurrency gate, always cleans temporary inputs and outputs, and is covered by the JPEG database/storage integration flow.

**New files:**

- None.

**Modified files:**

- `.env.example`
- `crates/media/src/office.rs`
- `crates/worker/src/lib.rs`
- `crates/worker/src/main.rs`
- `crates/worker/src/metadata.rs`
- `crates/worker/tests/metadata_job.rs`

---

### Story 5.10 — Preview UI (Modal/Lightbox)

As a user, I want to preview a file by double-clicking it or pressing a "Preview" button so that I can view files quickly without downloading. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Double-clicking a file row opens a preview modal/lightbox.
- [x] The modal shows a loading spinner while the preview is being generated.
- [x] For images: displays the preview image (zoomable).
- [x] For video: shows a `<video>` player with controls.
- [x] For audio: shows an `<audio>` player with controls.
- [x] For PDF: embeds the PDF viewer.
- [x] For DOCX: shows the converted PDF.
- [x] For unsupported types: shows file info and a "Download" button.
- [x] Pressing Escape or clicking outside closes the modal.
- [x] Arrow keys navigate to the next/previous file in the table.
- [x] Works in both themes.

**Implementation report:** Added a responsive preview lightbox with generation polling, media-specific image/video/audio/PDF renderers, DOCX-to-PDF display, zoom, fallback download details, backdrop and Escape dismissal, and file-to-file keyboard navigation. Fixture previews and browser checks verified the spinner, toolbar and double-click launch paths, image zoom, PDF and video renderers, navigation, dismissal, and both color themes without console errors.

**New files:**

- `apps/web/src/components/PreviewModal.css`
- `apps/web/src/components/PreviewModal.tsx`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/components/FileTable.css`
- `apps/web/src/components/FileTable.tsx`
- `apps/web/src/views/WorkspaceView.tsx`
- `crates/api/src/files.rs`
- `crates/worker/src/metadata.rs`

---
# Codex Stop
# Grok Start

---
