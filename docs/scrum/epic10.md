# Epic 10 — Queue Durability & Configuration Hygiene


**Goal:** The job queue stays fast as the library grows, and fixed paths become configuration.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 10.1 — Job Queue Indexes

As an operator, I want the job queue's hot queries indexed so that claim latency does not degrade as the jobs table grows. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] A migration adds an index supporting `claim_job`'s predicate and ordering — `(job_type, state, priority DESC, created_at, id)`, or a partial index restricted to `state = 'pending'`.
- [x] A migration adds an index supporting the lease reaper's `WHERE state = 'leased' AND lease_expires_at < now()`.
- [x] `EXPLAIN ANALYZE` of `claim_job` against a table seeded with 100,000 completed jobs shows an index scan rather than a sequential scan; the before and after plans are recorded in `docs/performance.md`.
- [x] The existing `jobs_active_type_target_unique` partial unique index is retained.
- [x] Down migrations drop the new indexes.

**Implementation report:** Migration 0028 adds two partial indexes sized by live work rather than retained history. `jobs_claim_pending_idx` covers job type, origin, priority, creation time, and id for pending rows, matching the worker's foreground, repair, and backfill claim lookups. `jobs_expired_lease_idx` orders only leased rows by expiry for the reaper. The down migration drops both, while the existing active-job uniqueness index is untouched.

The benchmark used PostgreSQL 17.10 and a rollback-only fixture containing 100,000 completed metadata jobs plus one pending job. The real pre-migration baseline already selected the broad `jobs_claim_origin_idx`; after migration PostgreSQL selected `jobs_claim_pending_idx`. Both returned the single live row in 0.036 ms, but the broad index occupied 7,768 kB while the partial live-work index occupied 56 kB. The expired-lease plan selected its new 8 kB index and executed in 0.011 ms. `docs/performance.md` records the plans, buffers, sizes, date, fixture, and reproduction guidance rather than presenting a misleading sequential-scan baseline. A migration integration test confirms both indexes and `jobs_active_type_target_unique` exist with their intended partial/unique definitions.

**New files:**

- `crates/db/migrations/0028_job_queue_indexes.down.sql`
- `crates/db/migrations/0028_job_queue_indexes.up.sql`
- `crates/db/tests/job_queue_indexes.rs`

**Modified files:**

- `crates/db/src/lib.rs`
- `docs/performance.md`
- `docs/scrum/epic10.md`
- `scrum.md`

---

### Story 10.2 — Job Retention & Purge

As an operator, I want completed jobs purged on a retention schedule so that the jobs table does not grow without bound. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] A retention policy is decided and recorded — for example, delete `completed` jobs after 7 days and retain `failed` and `cancelled` jobs longer for the Actionable Errors tab.
- [x] A purge loop runs in the worker alongside the existing `trash_cleanup_loop`, batched like `TRASH_CLEANUP_BATCH` so a large backlog does not lock the table.
- [x] Retention windows are configurable by environment variable with documented defaults.
- [x] The purge never removes a job in `pending` or `leased` state, and never removes a failed job still surfaced in the Actionable Errors tab.
- [x] The purge logs a count when it removes rows, consistent with the existing cleanup loops.
- [x] Test: jobs older than the retention window are purged while recent, pending, leased, and surfaced-failed jobs are retained.
- [x] `docs/performance.md` documents the expected steady-state jobs table size for a library of 100,000 files.

**Implementation report:** The retention policy is per outcome rather than uniform, because the two kinds of finished row have different value. A `completed` or `skipped` job is a receipt nobody re-reads once the artifact it produced exists; a `failed` or `cancelled` job carries `last_error` and the attempt count, which is exactly what an operator triaging a bad import or a parser regression reads. Successes are therefore dropped after `JOB_RETENTION_COMPLETED_DAYS` (7) and failures kept for `JOB_RETENTION_FAILED_DAYS` (30), both configurable and documented in `.env.example` and `docker-compose.prod.yml`.

`job_purge_loop` runs alongside `trash_cleanup_loop` on the same hourly cadence and takes one `JOB_PURGE_BATCH` bite per tick, so a table neglected for months drains over hours rather than locking in a single statement. The delete uses `FOR UPDATE SKIP LOCKED` so it never contends with job claiming. It logs a count and both retention windows when it removes rows, matching the existing cleanup loops.

Three things are never deleted, each for a specific reason. **Pending and leased jobs at any age**, because a leased job whose worker died is recovered by lease expiry — deleting it would strand the work with nothing left to recover from. **Failed jobs behind an unresolved `failed` import entry**, because that entry is what the Actionable Errors tab lists and its retry needs the job's error context; once the entry is resolved the job becomes ordinary history and is purged normally. **Anything beyond the batch**, so the first run against a long-neglected table is bounded.

One test is worth naming: purging a job asserts its *node* still exists. The job is bookkeeping and the file it processed is the point, so a cascade from one to the other would be a data-loss bug rather than a cleanup.

`docs/performance.md` gains a steady-state section. The useful insight there is that the table is governed by throughput rather than library size — it holds a retention window's worth of work, not one row per file — so a 100,000-file library idles near zero and peaks around 60 MB in a heavy week. The number that motivated this story is the backfill: a 691,000-message email campaign would leave roughly 4.8 million completed rows and about 2 GB behind, and they would still be there months later.

**New files:**

- `crates/db/tests/job_retention.rs`

**Modified files:**

- `.env.example`
- `crates/db/src/lib.rs`
- `crates/worker/src/lib.rs`
- `docker-compose.prod.yml`
- `docs/performance.md`
- `docs/scrum/epic10.md`
- `scrum.md`

---

### Story 10.3 — Configurable Import Watch Root

As an operator, I want the import watch directory to come from configuration so that the import path is not compiled into the binary. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] The two hardcoded `/mnt/ext/watch` literals in `crates/api/src/lib.rs` (the `imports::router` argument and `recover_watched_imports`) are replaced by a `Config` field.
- [x] A new environment variable (for example `IMPORT_WATCH_ROOT`) is read by `Config::from_env`, defaults to `/mnt/ext/watch` for compatibility, and is validated the way `STORAGE_ROOT` is.
- [x] `docker-compose.prod.yml`, `docker-compose.dev.yml`, and `.env.example` set the variable explicitly rather than relying on the bind-mount path matching a compiled-in constant.
- [x] The configuration shape does not preclude multiple source-to-destination mappings later (see the import questions in `deferred.md`); a note records how it would extend.
- [x] A missing or unreadable watch root logs a warning and disables import rather than failing startup, preserving today's behavior.
- [x] Tests: config parsing covers the default, an override, and invalid values.

**Implementation report:** `Config` now owns `import_watch_root`, loaded from `IMPORT_WATCH_ROOT`, defaulting to `/mnt/ext/watch`, and rejecting empty values. Startup records the configured path, probes it with `read_dir`, and passes that same path to interrupted-import recovery and the import router. A missing path, regular file, or unreadable directory emits a structured warning and omits the import routes while the rest of the API continues to start.

Production Compose explicitly maps the configured container path to `/mnt/ext/watch`; the development Compose metadata and `.env.example` use `./.data/import` for host-run development. ADR 0004 records that this independent field can later become a list of `{ source_root, destination_folder_id }` mappings without coupling imports to managed storage, and the deferred question and known limitation now use the configurable terminology.

Unit tests cover the compatibility default, a relative override, an empty invalid value, a readable directory, a missing directory, and a regular file. API and database clippy remain warning-free, and the focused configuration and migration tests pass.

**New files:**

- None.

**Modified files:**

- `.env.example`
- `crates/api/src/config.rs`
- `crates/api/src/lib.rs`
- `deferred.md`
- `docker-compose.dev.yml`
- `docker-compose.prod.yml`
- `docs/decisions/0004-watched-folder-import.md`
- `docs/known-limitations.md`
- `docs/scrum/epic10.md`
- `scrum.md`

**Completion verification (2026-08-04):** The complete serial Rust workspace test suite and warning-denied workspace Clippy passed. Focused database verification proved both partial queue indexes and retained active-job uniqueness, all six retention invariants, and the configurable import-root default, override, invalid-value, missing-directory, regular-file, and readable-directory cases. Migration 28 and its down migration contain the paired index create/drop operations, while the split scrum mirror retains every checked acceptance criterion and implementation report.

---
