# Epic 10 — Queue Durability & Configuration Hygiene


**Goal:** The job queue stays fast as the library grows, and fixed paths become configuration.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 10.1 — Job Queue Indexes

As an operator, I want the job queue's hot queries indexed so that claim latency does not degrade as the jobs table grows. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] A migration adds an index supporting `claim_job`'s predicate and ordering — `(job_type, state, priority DESC, created_at, id)`, or a partial index restricted to `state = 'pending'`.
- [ ] A migration adds an index supporting the lease reaper's `WHERE state = 'leased' AND lease_expires_at < now()`.
- [ ] `EXPLAIN ANALYZE` of `claim_job` against a table seeded with 100,000 completed jobs shows an index scan rather than a sequential scan; the before and after plans are recorded in `docs/performance.md`.
- [ ] The existing `jobs_active_type_target_unique` partial unique index is retained.
- [ ] Down migrations drop the new indexes.

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

- [ ] The two hardcoded `/mnt/ext/watch` literals in `crates/api/src/lib.rs` (the `imports::router` argument and `recover_watched_imports`) are replaced by a `Config` field.
- [ ] A new environment variable (for example `IMPORT_WATCH_ROOT`) is read by `Config::from_env`, defaults to `/mnt/ext/watch` for compatibility, and is validated the way `STORAGE_ROOT` is.
- [ ] `docker-compose.prod.yml`, `docker-compose.dev.yml`, and `.env.example` set the variable explicitly rather than relying on the bind-mount path matching a compiled-in constant.
- [ ] The configuration shape does not preclude multiple source-to-destination mappings later (see the import questions in `deferred.md`); a note records how it would extend.
- [ ] A missing or unreadable watch root logs a warning and disables import rather than failing startup, preserving today's behavior.
- [ ] Tests: config parsing covers the default, an override, and invalid values.

---
