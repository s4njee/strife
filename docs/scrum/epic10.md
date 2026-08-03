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

- [ ] A retention policy is decided and recorded — for example, delete `completed` jobs after 7 days and retain `failed` and `cancelled` jobs longer for the Actionable Errors tab.
- [ ] A purge loop runs in the worker alongside the existing `trash_cleanup_loop`, batched like `TRASH_CLEANUP_BATCH` so a large backlog does not lock the table.
- [ ] Retention windows are configurable by environment variable with documented defaults.
- [ ] The purge never removes a job in `pending` or `leased` state, and never removes a failed job still surfaced in the Actionable Errors tab.
- [ ] The purge logs a count when it removes rows, consistent with the existing cleanup loops.
- [ ] Test: jobs older than the retention window are purged while recent, pending, leased, and surfaced-failed jobs are retained.
- [ ] `docs/performance.md` documents the expected steady-state jobs table size for a library of 100,000 files.

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
