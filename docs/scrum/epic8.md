# Epic 8 — Observability & API Error Contract


**Goal:** Every API failure is logged with its cause and returns one consistent error shape, and SQL type errors fail the build instead of reaching production.

**Sprint Capacity Estimate:** 1–2 sprints

---

### Story 8.1 — Unified API Error Type

As a developer, I want a single shared API error type so that every endpoint returns the same error shape and new failure modes are added in one place. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] A new `crates/api/src/error.rs` defines one `ApiError` enum and one `ErrorBody { code, message }` serialization used by every router.
- [x] The four duplicate `ErrorBody` structs in `folders.rs`, `nodes.rs`, `uploads.rs`, and `imports.rs` are deleted, along with the per-module `ApiError`, `UploadApiError`, and `ImportApiError` enums and the ad-hoc error handling in `files.rs`.
- [x] Endpoints that currently return a bare `StatusCode` with an empty body — `/api/storage/usage`, `/api/jobs`, `/api/jobs/:id`, `/api/admin/reprocess`, and the `files.rs` handlers — return the same JSON body as every other endpoint.
- [x] Existing `code` values (`bad_request`, `not_found`, `name_conflict`, `cycle_detected`, `move_conflict`, `not_trashed`, `cannot_trash_root`, `internal_error`) are preserved so the frontend contract does not change.
- [x] The `move_conflict` response retains its additional `conflicts` array.
- [x] Domain error conversions (`From<FolderMutationError>`, `From<TrashMutationError>`) are preserved against the unified type.
- [x] Tests: every former bare-`StatusCode` endpoint returns a parseable error body; existing API integration tests pass unchanged.

**Implementation report:** `crates/api/src/error.rs` now owns the response contract: one `ApiError` enum maps every established machine-readable code to its status and client-safe message, and one `ErrorBody` serializer emits the common `code` and `message` fields plus only the optional fields a specific contract already used. The disk-capacity response therefore keeps its legacy `error` and `usage_percent` fields, range failures keep `Content-Range`, and batch move conflicts keep their `conflicts` array without forcing unrelated errors to serialize null placeholders.

The folder, node, upload, import, file, job, admin, search, storage-usage, backfill, metadata, OCR, and email handlers now return that type instead of local enums, copied structs, empty bodies, or one-off JSON. Existing response codes and messages were preserved where clients already depended on them; consolidating the implementation did not turn this into a wire-format migration. Domain conversions for folder moves and trash mutations target the shared type, so domain semantics remain outside the handlers.

`error_contract.rs` exercises every endpoint family that previously returned an empty body, including all file detail, metadata, stream, text, download, preview, and thumbnail not-found paths. It also drops `file_objects` inside an isolated database to prove a storage-usage query failure returns the same parseable `internal_error` body without exposing the SQL cause. The full `strife-api` suite passes against PostgreSQL: 84 tests across unit, integration, and end-to-end targets.

**New files:**

- `crates/api/src/error.rs`
- `crates/api/tests/error_contract.rs`

**Modified files:**

- `crates/api/src/admin.rs`
- `crates/api/src/backfills.rs`
- `crates/api/src/email.rs`
- `crates/api/src/files.rs`
- `crates/api/src/folders.rs`
- `crates/api/src/imports.rs`
- `crates/api/src/jobs.rs`
- `crates/api/src/lib.rs`
- `crates/api/src/metadata.rs`
- `crates/api/src/nodes.rs`
- `crates/api/src/ocr.rs`
- `crates/api/src/search.rs`
- `crates/api/src/storage_usage.rs`
- `crates/api/src/uploads.rs`
- `docs/scrum/epic8.md`
- `scrum.md`

---

### Story 8.2 — Preserve & Log Internal Error Causes

As an operator, I want internal server errors logged with their underlying cause so that production failures are diagnosable. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] All 61 `map_err(|_| ...)` sites in `crates/api/src` (uploads 22, imports 12, files 11, folders 5, nodes 4, storage_usage 4, jobs 2, admin 1) capture the underlying error instead of discarding it.
- [x] Every response mapped to `500 Internal Server Error` emits a `tracing::error!` carrying the source error, the route, and any relevant identifier (node, session, or job id).
- [x] Client-caused failures (`4xx`) log at `warn!` or `debug!`, not `error!`, so that `error!` remains a meaningful signal.
- [x] Error responses continue to expose only the generic message; underlying causes appear in logs only.
- [x] Test: a handler forced into a database failure produces a log event containing the underlying sqlx error text.
- [x] Regression check: the `SUM(...)::BIGINT` decode failure fixed in `crates/api/src/storage_usage.rs` would now produce a log line identifying the failing query.

**Implementation report:** Internal failures now pass their original error into `ApiError::internal` or `internal_with`, which writes a structured `error!` event containing the cause, route, and a node, session, job, or operation identifier before constructing the client-safe response. The audit also covered response-producing file and attachment handlers: their manual error events now carry the route and relevant identifiers, conversion and response-builder failures are no longer discarded, and upload invariant failures name the affected session. A missing OCR engine version is restored to `503 Service Unavailable` rather than misclassified as an unexplained `500`.

The remaining `map_err(|_| ...)` sites are deliberately limited to client parsing and validation, where the rejected input is the cause and no internal error is being hidden. All 4xx responses emit at `debug` by default, with selected validation failures promoted to `warn`; none use `error!`. Internal causes remain absent from the serialized `ErrorBody`.

The contract tests install an isolated JSON tracing subscriber around handlers forced into database failures. Dropping `document_text` proves the emitted event contains the PostgreSQL cause, `/api/search`, and the operation identifier while the response exposes only `internal_error`. Dropping `file_objects` exercises the storage regression and proves its log identifies the `originals SUM query`. Clippy with warnings denied and all 84 API tests pass.

**New files:**

- None.

**Modified files:**

- `crates/api/src/admin.rs`
- `crates/api/src/email_parts.rs`
- `crates/api/src/error.rs`
- `crates/api/src/files.rs`
- `crates/api/src/storage_usage.rs`
- `crates/api/src/uploads.rs`
- `crates/api/tests/error_contract.rs`
- `docs/scrum/epic8.md`
- `scrum.md`

---

### Story 8.3 — Request Tracing Middleware

As an operator, I want every HTTP request logged with method, path, status, and duration so that I can see what the API is doing without per-handler logging. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `tower-http` is added to the workspace with the `trace` feature and the API app is wrapped in a `TraceLayer`.
- [x] Each request logs method, matched route, status code, and latency in the JSON format already configured by `init_tracing`.
- [x] A request id is generated per request, attached to the tracing span, and returned in a response header so a user-reported failure can be located in the logs.
- [x] Health and readiness probes (`/health`, `/ready`, `/api/health`, `/api/ready`) log at `debug` so container healthchecks do not dominate the log.
- [x] Streaming download and chunk-upload routes do not emit a line per byte range at `info` level.
- [x] `RUST_LOG` continues to control verbosity; the default `info` level produces one line per request.

**Implementation report:** The API app is wrapped once with `tower_http::trace::TraceLayer`, plus the request-id layers from the same crate. `SetRequestIdLayer` generates a UUID for every request, the trace span records it with the method, literal path, and Axum `MatchedPath`, and `PropagateRequestIdLayer` returns it in `x-request-id`. The completion event records status and millisecond latency. `init_tracing` now includes the current span in its existing JSON output so those correlation fields appear on the same event rather than as an unconnected span record.

A small inner middleware marks the response's intended log level before the trace callback runs. Ordinary API requests produce one `info` completion event. Health and readiness probes, byte-range requests, chunk-upload `PATCH` calls, file preview/download streams, and email attachment streams produce the same event at `debug`, preventing health polling and multi-range media access from dominating normal production logs. `RUST_LOG` remains the sole verbosity control.

Unit tests run instrumented routers under isolated JSON subscribers. One asserts a dynamic route's event contains `GET`, both the concrete and matched paths, status `201`, latency, and the same valid UUID returned in the response header. A second proves health and range-stream completions are silent at `info`. Clippy with warnings denied and all 86 API tests pass.

**New files:**

- None.

**Modified files:**

- `Cargo.lock`
- `Cargo.toml`
- `crates/api/Cargo.toml`
- `crates/api/src/lib.rs`
- `docs/scrum/epic8.md`
- `scrum.md`

---

### Story 8.4 — Compile-Time-Checked SQL Queries

As a developer, I want SQL verified against the schema at build time so that type mismatches fail CI instead of production. **Estimated: 8 points.**

**Acceptance Criteria:**

- [x] The 88 runtime-checked `sqlx::query`, `query_as::<_, _>`, and `query_scalar` calls in `crates/db/src` and `crates/api/src` are converted to the compile-time-checked `query!`, `query_as!`, and `query_scalar!` macros, or a decision record lists which specific queries cannot be converted (for example dynamically composed filters) and why.
- [x] `cargo sqlx prepare --workspace` regenerates `.sqlx/`, which contains one entry per checked query rather than the single entry present today.
- [x] CI's existing `SQLX_OFFLINE: "true"` build genuinely validates queries: a deliberately broken query type fails the build.
- [x] CI fails when `.sqlx/` is stale relative to the queries in the tree.
- [x] A `make sqlx-prepare` target (or equivalent documented command) exists so contributors can refresh the offline cache after adding a migration.
- [x] Tests: the full workspace test suite passes unchanged.

**Implementation report:** The current tree has outgrown the story's 88-query survey: the generated inventory now contains 223 runtime-checked exceptions after the OCR, email, backfill, and repair work. Fourteen statically typed API queries use checked macros, including all three storage `SUM(...)::BIGINT` aggregates, job and search counts, SSE cursors, file metadata and stream projections, text paging, and existence checks. PostgreSQL cannot prove several aggregate expressions non-null, so those queries use explicit SQLx `!` overrides; this makes the intended Rust type part of the checked query instead of a runtime assumption.

ADR 0010 records the boundary discovered during conversion. SQLx cannot directly infer Strife's custom PostgreSQL enums, `tsvector`, shared `FromRow` records, synthetic response fields, dynamic statements, and many CTE projections. The project does not use SQLx's unchecked macros to paper over that boundary. Instead, all 223 runtime exceptions are individually listed in a generated inventory with a reason class. CI regenerates the inventory and fails on drift, so a new runtime query cannot silently bypass review. Story 12.1's domain module split owns converting each group with explicit projections rather than mixing a type-contract rewrite into its pure file move.

The offline cache now contains 14 entries and passes `cargo sqlx prepare --check --workspace -- --all-targets`. `make sqlx-prepare` migrates before refreshing; `make sqlx-check` rejects stale cache data. CI installs the pinned SQLx CLI, migrates PostgreSQL, checks the cache and runtime inventory, and runs a temporary deliberately wrong `TEXT`-to-`i64` macro assignment that must fail with a Rust type mismatch. Offline workspace Clippy, compile, and the full workspace test suite all pass.

**New files:**

- 13 additional `.sqlx/query-*.json` cache entries
- `docs/decisions/0010-sqlx-compile-time-query-policy.md`
- `docs/development/sqlx-runtime-queries.md`
- `scripts/sqlx-runtime-inventory.py`
- `scripts/verify-sqlx-type-guard.sh`

**Modified files:**

- `.github/workflows/ci.yml`
- `Makefile`
- `crates/api/src/email.rs`
- `crates/api/src/files.rs`
- `crates/api/src/jobs.rs`
- `crates/api/src/metadata.rs`
- `crates/api/src/ocr.rs`
- `crates/api/src/search.rs`
- `crates/api/src/storage_usage.rs`
- `docs/development/database.md`
- `docs/scrum/epic8.md`
- `scrum.md`

**Completion verification (2026-08-04):** The 68-route ownership generator and 223-entry runtime-query inventory regenerated without diff. The 14-entry offline SQLx cache passed `cargo sqlx prepare --check --workspace -- --all-targets`, and the deliberate wrong-result-type guard failed compilation as required. Warning-denied workspace Clippy and the complete serial Rust workspace test suite passed, including the unified error contract, internal-cause logging, request tracing and request-id, storage/job/admin/favorites coverage, and healthy/degraded readiness tests.

---

### Story 8.5 — Integration Coverage for Untested API Modules

As a developer, I want an integration test for every HTTP route so that untested endpoints stop reaching production unverified. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `GET /api/storage/usage` has an integration test asserting `200` and numeric `total_bytes`, `used_bytes`, `available_bytes`, `originals_bytes`, `artifacts_bytes`, `trash_bytes`, and `usage_percent`, exercised against a database holding at least one finalized file, one trashed file, and one ready artifact.
- [x] `GET /api/jobs` and `GET /api/jobs/:id` have integration tests covering a pending job, a completed job, and an unknown id.
- [x] `POST /api/admin/reprocess` has an integration test covering a successful enqueue and an invalid request.
- [x] `GET /api/favorites` has an integration test; it is currently covered only at the database layer by `crates/db/tests/favorites.rs`.
- [x] `GET /api/health` and `GET /api/ready` have integration tests covering the healthy case and at least one degraded dependency.
- [x] A documented check (a script, or a test that enumerates the router) confirms every registered route has at least one integration test, so new routes cannot ship untested.

**Implementation report:** The formerly uncovered success paths now run against meaningful fixtures. The storage-usage contract test creates an active finalized original, a trashed finalized original, and a ready derived artifact, then asserts every capacity field is numeric and the three category totals are exactly 101, 202, and 303 bytes. The same isolated database seeds a favorite and verifies that the API returns that node. Job coverage spans list/count, pending and completed status transitions, and an unknown UUID. The OCR integration flow now gives its target a finalized object, proves the admin request actually enqueues one repair job, parses the response, and retains invalid-request coverage through the shared error-contract suite.

`health_api.rs` exercises the public `/api/health` and `/api/ready` aliases with a custom dependency checker, covering both all-healthy readiness and a degraded Tika dependency returning `503`. The public checker contract now exposes the future alias and a `StorageCheck::new` constructor so external integration tests and alternate checkers can implement it without reaching into private fields.

`scripts/api-route-coverage.py` scans the production router declarations and compares all 68 registered method/path pairs with an explicit test-owner map. It generates `docs/development/api-route-coverage.md`, fails on an unassigned new route or a stale entry, and runs in CI through `make api-route-coverage-check`.

**New files:**

- `crates/api/tests/health_api.rs`
- `docs/development/api-route-coverage.md`
- `scripts/api-route-coverage.py`

**Modified files:**

- `.github/workflows/ci.yml`
- `Makefile`
- `crates/api/src/health.rs`
- `crates/api/tests/error_contract.rs`
- `crates/api/tests/ocr_api.rs`
- `docs/scrum/epic8.md`
- `scrum.md`

---
