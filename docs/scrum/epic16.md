# Epic 16 — OCR Status & Text UI


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
- [x] OCR adopts the shared `backfill_campaigns`, job origin/campaign fields, durable cursor, scheduler, priority policy, and renewable resource leases specified by [`backfill.md`](../backfill.md).
- [x] A read-only preflight reports OCR candidates by MIME family, file/page/size percentiles where discoverable without OCR, current text state, projected work, and estimated storage without enqueueing jobs.
- [x] A historical OCR campaign starts paused and records candidate snapshot criteria, batch size, maximum queued/running work, resource class, cursor, counts, timestamps, and initiating version.
- [x] Initial Orion defaults are 100 candidates per refill, at most 500 queued OCR jobs, one running OCR backfill job, and one shared `HEAVY_CPU` permit across OCR, email, and attachment backfills.
- [x] Foreground jobs outrank repair work, which outranks historical OCR; a fairness budget allows slow backfill progress without hiding new uploads, imports, metadata, previews, or deletions.
- [x] Pausing stops refills while leased work finishes; resuming continues from the durable cursor; cancelling prevents new claims without deleting completed OCR text.
- [x] The OCR page distinguishes foreground activity from campaigns and exposes candidate count, state, progress, throughput, ETA, limits, start/pause/resume/cancel controls, and canary results.
- [ ] Email body parsing completes before the full ordinary OCR campaign begins, and email attachment OCR uses the same OCR/shared-heavy permit after ordinary OCR unless an operator explicitly changes the documented sequence.
- [x] Tests cover inert deployment/startup, foreground processing while paused, canary limits, low-water refill, cross-pipeline mutual exclusion, priority/fairness, pause/resume/cancel, restart recovery, and multi-worker resource-lease enforcement.

**Implementation progress:** Registered the OCR adapter on the shared coordinator and implemented historical candidate selection end to end. Candidate selection, enqueue, and `(created_at, id)` cursor advance share one transaction, so an interrupted refill can neither skip nor repeat a file. Candidates are active finalized files whose extracted `detected_mime` is an OCR input and whose document text is absent or from a different engine version; files still awaiting metadata are counted separately rather than guessed at from their filename. Added a read-only preflight endpoint reporting candidates and byte percentiles per MIME family, and OCR page controls for preflight, paused campaign creation from a reviewed report, resume, pause, and cancel. Two safety guards are enforced in code: an unprepared campaign has no frozen snapshot and refuses to enumerate, and a worker with no verified Tesseract refuses to refill rather than treating every file as a version mismatch and enqueueing the whole library.

The OCR page now adds live pending/running/remaining counts, recent throughput, an estimated completion time, and recorded canary results, refreshed every 15 seconds. Initial campaign creation uses an exact 100-item canary cap; the shared coordinator stops enqueueing at that boundary and automatically returns the campaign to paused after its queue drains. Canary result records capture stage, throughput, p50/p95 duration, failures, CPU, memory, temperature, I/O wait, database growth, and approval without a free-form field that could leak file data.

Test coverage now includes the previously missing restart and foreground-isolation cases: a new coordinator resumes from the durable cursor without duplicates, paused history does not prevent a foreground OCR claim, and an exact 100-of-105 canary auto-pauses after drain. The focused suite also retains inert startup, low-water refill, exhaustion, pause/resume/cancel, mutual exclusion, priority/fairness, and multi-worker lease coverage. Story 16.6 remains open on only the production sequencing criterion: the email body campaign must complete before ordinary OCR, then attachment OCR follows ordinary OCR unless the operator records a documented sequence change. This is deliberately an operational gate in [`backfill.md`](../backfill.md), not an automatic deployment side effect.

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
- `crates/api/src/backfills.rs`
- `crates/api/src/ocr.rs`
- `crates/api/tests/backfills_api.rs`
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
> At the ~30 points/sprint velocity assumed in [`scrum.md`](../../scrum.md), this is roughly **3 sprints** of work on top of the 105 stories already planned there. Story 16.6 is the production backfill amendment required by the later email plan.

> [!IMPORTANT]
> Suggested order: **13.1 and 13.2 first** — the schema is the contract every other story writes against, and changing it after the worker and UI depend on it is the expensive mistake here. **13.3 before Epic 14**, because embedded-text detection determines how much OCR work actually exists and may substantially reduce it. Epic 15 can proceed in parallel with Epic 16 once text is landing in the database.

> Feature deployment and historical processing are separate. Ship OCR support with historical campaigns paused, process new files as foreground work, then follow [`backfill.md`](../backfill.md) for preflight, email-first canaries, and the later historical OCR campaign.

> [!WARNING]
> Two questions from `deferred.md` are answered only provisionally and are carried as follow-ups by Story 13.1: the concrete page, pixel, time, memory, and output-size limits ("this may need to be profiled, use sensible defaults for now"), and the reprocessing policy when OCR models or tool versions change, which Story 14.6 makes possible but does not make automatic. Neither should be treated as settled.
