# Epic 8 — Observability & API Error Contract


**Goal:** Every API failure is logged with its cause and returns one consistent error shape, and SQL type errors fail the build instead of reaching production.

**Sprint Capacity Estimate:** 1–2 sprints

---

### Story 8.1 — Unified API Error Type

As a developer, I want a single shared API error type so that every endpoint returns the same error shape and new failure modes are added in one place. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] A new `crates/api/src/error.rs` defines one `ApiError` enum and one `ErrorBody { code, message }` serialization used by every router.
- [ ] The four duplicate `ErrorBody` structs in `folders.rs`, `nodes.rs`, `uploads.rs`, and `imports.rs` are deleted, along with the per-module `ApiError`, `UploadApiError`, and `ImportApiError` enums and the ad-hoc error handling in `files.rs`.
- [ ] Endpoints that currently return a bare `StatusCode` with an empty body — `/api/storage/usage`, `/api/jobs`, `/api/jobs/:id`, `/api/admin/reprocess`, and the `files.rs` handlers — return the same JSON body as every other endpoint.
- [ ] Existing `code` values (`bad_request`, `not_found`, `name_conflict`, `cycle_detected`, `move_conflict`, `not_trashed`, `cannot_trash_root`, `internal_error`) are preserved so the frontend contract does not change.
- [ ] The `move_conflict` response retains its additional `conflicts` array.
- [ ] Domain error conversions (`From<FolderMutationError>`, `From<TrashMutationError>`) are preserved against the unified type.
- [ ] Tests: every former bare-`StatusCode` endpoint returns a parseable error body; existing API integration tests pass unchanged.

---

### Story 8.2 — Preserve & Log Internal Error Causes

As an operator, I want internal server errors logged with their underlying cause so that production failures are diagnosable. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] All 61 `map_err(|_| ...)` sites in `crates/api/src` (uploads 22, imports 12, files 11, folders 5, nodes 4, storage_usage 4, jobs 2, admin 1) capture the underlying error instead of discarding it.
- [ ] Every response mapped to `500 Internal Server Error` emits a `tracing::error!` carrying the source error, the route, and any relevant identifier (node, session, or job id).
- [ ] Client-caused failures (`4xx`) log at `warn!` or `debug!`, not `error!`, so that `error!` remains a meaningful signal.
- [ ] Error responses continue to expose only the generic message; underlying causes appear in logs only.
- [ ] Test: a handler forced into a database failure produces a log event containing the underlying sqlx error text.
- [ ] Regression check: the `SUM(...)::BIGINT` decode failure fixed in `crates/api/src/storage_usage.rs` would now produce a log line identifying the failing query.

---

### Story 8.3 — Request Tracing Middleware

As an operator, I want every HTTP request logged with method, path, status, and duration so that I can see what the API is doing without per-handler logging. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `tower-http` is added to the workspace with the `trace` feature and the API app is wrapped in a `TraceLayer`.
- [ ] Each request logs method, matched route, status code, and latency in the JSON format already configured by `init_tracing`.
- [ ] A request id is generated per request, attached to the tracing span, and returned in a response header so a user-reported failure can be located in the logs.
- [ ] Health and readiness probes (`/health`, `/ready`, `/api/health`, `/api/ready`) log at `debug` so container healthchecks do not dominate the log.
- [ ] Streaming download and chunk-upload routes do not emit a line per byte range at `info` level.
- [ ] `RUST_LOG` continues to control verbosity; the default `info` level produces one line per request.

---

### Story 8.4 — Compile-Time-Checked SQL Queries

As a developer, I want SQL verified against the schema at build time so that type mismatches fail CI instead of production. **Estimated: 8 points.**

**Acceptance Criteria:**

- [ ] The 88 runtime-checked `sqlx::query`, `query_as::<_, _>`, and `query_scalar` calls in `crates/db/src` and `crates/api/src` are converted to the compile-time-checked `query!`, `query_as!`, and `query_scalar!` macros, or a decision record lists which specific queries cannot be converted (for example dynamically composed filters) and why.
- [ ] `cargo sqlx prepare --workspace` regenerates `.sqlx/`, which contains one entry per checked query rather than the single entry present today.
- [ ] CI's existing `SQLX_OFFLINE: "true"` build genuinely validates queries: a deliberately broken query type fails the build.
- [ ] CI fails when `.sqlx/` is stale relative to the queries in the tree.
- [ ] A `make sqlx-prepare` target (or equivalent documented command) exists so contributors can refresh the offline cache after adding a migration.
- [ ] Tests: the full workspace test suite passes unchanged.

---

### Story 8.5 — Integration Coverage for Untested API Modules

As a developer, I want an integration test for every HTTP route so that untested endpoints stop reaching production unverified. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `GET /api/storage/usage` has an integration test asserting `200` and numeric `total_bytes`, `used_bytes`, `available_bytes`, `originals_bytes`, `artifacts_bytes`, `trash_bytes`, and `usage_percent`, exercised against a database holding at least one finalized file, one trashed file, and one ready artifact.
- [ ] `GET /api/jobs` and `GET /api/jobs/:id` have integration tests covering a pending job, a completed job, and an unknown id.
- [ ] `POST /api/admin/reprocess` has an integration test covering a successful enqueue and an invalid request.
- [ ] `GET /api/favorites` has an integration test; it is currently covered only at the database layer by `crates/db/tests/favorites.rs`.
- [ ] `GET /api/health` and `GET /api/ready` have integration tests covering the healthy case and at least one degraded dependency.
- [ ] A documented check (a script, or a test that enumerates the router) confirms every registered route has at least one integration test, so new routes cannot ship untested.

---
