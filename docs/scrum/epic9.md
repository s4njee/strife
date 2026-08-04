# Epic 9 — Production Deployment & Process Lifecycle


**Goal:** The production deployment that already exists in the working tree is committed, documented, and survives a restart without severing in-flight work.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 9.1 — Commit Production Deployment Assets

As a project owner, I want the production deployment files committed so that the deployed configuration is version-controlled and reproducible. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] `docker-compose.prod.yml`, `deploy/docker/backend.Dockerfile`, `deploy/docker/web.Dockerfile`, `deploy/docker/Caddyfile`, `deploy/orion/strife.service`, `deploy/orion/README.md`, `.dockerignore`, and `scripts/import-icloud.sh` are committed.
- [x] The pending `SUM(...)::BIGINT` fix in `crates/api/src/storage_usage.rs` is committed together with the regression test from Story 8.5.
- [x] `.env.example` documents every variable the production stack requires, including `POSTGRES_PASSWORD`, `STRIFE_IMAGE_TAG`, and `STRIFE_REVISION`.
- [x] Host secret files (`/etc/strife/postgres.env`, `/etc/strife/revision.env`) are documented but not committed, and `.gitignore` prevents accidental inclusion.
- [x] `deploy/orion/README.md` records the host layout (`/srv/strife/storage`, `/srv/strife/postgres`, `/srv/strife/import`, `/opt/strife`) and the install, upgrade, and rollback procedures.
- [x] A CI job builds both deployment images so a broken Dockerfile fails the build.

**Implementation report:** The production Compose, container, Caddy, Orion systemd, and import assets are version-controlled, with all required runtime variables documented in `.env.example` and host-only secret files excluded by the repository ignore policy. The Orion runbook now specifies the four host paths plus install, migration-first upgrade, readiness verification, and revision-based rollback. CI builds the migration, API, worker, and web images on native x86-64 and ARM64 runners and verifies that the worker image contains Tesseract with English language data. The production Compose configuration also passes interpolation and schema validation with placeholder secrets.

**Operational validation (2026-08-03):** Orion deployed immutable ARM64 revision `f0352e2` from `/opt/strife-releases/f0352e2` through `/opt/strife-current`, leaving the dirty legacy `/opt/strife` checkout untouched. Before the rollout, PostgreSQL received a verified custom-format dump and every `strife` ZFS dataset received a recursive `pre-ocr-cd719a0` snapshot. Migration 28 completed, the API reported PostgreSQL, storage, and Tika ready, and metadata remained drained at 678,380 completed, 403 failed, and zero remaining. The OCR coordinator was enabled only after an inert health check. Its frozen 40,847-file preflight campaign enqueued exactly the authorized 100-file canary, processed 97 files, intentionally skipped three PDFs with usable embedded text, failed none, and auto-paused with zero queued or running work. No email campaign was started. Orion peaked at 54.3 C during sampled execution and returned to 47.7 C with 2.1 GiB available memory. Docker reported that the host kernel does not enforce the Compose worker memory limit; OCR therefore remains at one heavy-CPU permit while that host-level limitation is tracked during later stages.

---

### Story 9.2 — API Graceful Shutdown

As an operator, I want the API to drain in-flight requests on SIGTERM so that restarts do not sever active uploads and downloads. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `axum::serve(...)` in `crates/api/src/lib.rs` uses `.with_graceful_shutdown(...)` with SIGTERM and Ctrl-C handling matching the pattern already implemented in `crates/worker/src/lib.rs`.
- [x] The background task spawned by `spawn_upload_cleanup` observes the same shutdown signal and exits cleanly rather than being aborted mid-sweep.
- [x] In-flight chunk uploads and range downloads complete before the process exits, subject to a bounded drain timeout.
- [x] The `api` service in `docker-compose.prod.yml` sets a `stop_grace_period` consistent with that drain timeout; today only `worker` sets one (`10m`) and `api` falls back to the 10-second default.
- [x] A shutdown log line records how many requests were drained.
- [x] Test: a request in flight when SIGTERM is delivered completes with a success status.
- [x] Test: restarting the stack during an active multi-chunk upload leaves the session resumable rather than failed.

**Implementation report:** API shutdown now shares one signal across Axum and upload-session cleanup, accepts SIGTERM or Ctrl-C, and bounds HTTP draining at 30 seconds before joining cleanup for a further five seconds. A response-body guard distinguishes requests active at signal time from fully drained and prematurely dropped bodies, and the final structured log reports all three counts. Production Compose grants the API 45 seconds. Unit coverage holds an in-flight streaming response across shutdown and verifies successful completion; the upload edge-case integration test drops and rebuilds the router between chunks and proves the persisted session remains resumable.

---

### Story 9.3 — ARM64 Build & Test in CI

As a developer, I want CI to build and test the deployment architecture so that ARM64 regressions are caught before deployment. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] The CI matrix in `.github/workflows/ci.yml` includes `aarch64-unknown-linux-gnu` alongside `x86_64-unknown-linux-gnu`.
- [x] The ARM64 job runs clippy, build, and the test suite under the same `-D warnings` gate.
- [x] Container images are built for `linux/arm64` in CI, matching the claim already made in Story 7.4.
- [x] Checks in `scripts/validate-arm64.sh` that CI now covers are removed or marked CI-covered, leaving only genuinely device-specific steps (OOM sampling, tool availability on the Pi).
- [x] Job runtime is documented; if emulation is too slow for every push, the ARM64 job runs on merge to `main` and the split is recorded in `docs/development`.

**Implementation report:** CI uses GitHub's native `ubuntu-24.04-arm` runner beside `ubuntu-24.04`, applying the same formatting, SQLx, route-ownership, clippy `-D warnings`, build, test, and frontend gates to both architectures. A second native matrix builds every production image on each architecture, avoiding emulation. `docs/development/arm64-ci.md` records the expected 15–30 minute warm-cache runtime, the runner's preview status, and a main-only fallback if availability becomes unreliable. The device script is reduced to architecture, extractor/OCR tool, memory, and kernel OOM observations.

---

### Story 9.4 — Documentation Reconciliation with Shipped Deployment

As a project owner, I want documentation to match what is actually deployed so that the plan is trustworthy going into v2. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] `docs/known-limitations.md` no longer states "Development-oriented Docker Compose; production packaging deferred"; it describes the shipped production stack and its real remaining limits (no TLS, single host, LAN-only).
- [x] `README.md` is corrected where it states packaged deployment is a v2 concern (the deployment-direction row and the v1 non-goals section).
- [x] `deferred.md` removes or rewrites the now-answered question "What should a production-ready Docker Compose bundle include?" and retains the genuinely open ones (TLS, Kubernetes, native binaries, published images).
- [x] `docs/setup.md` documents production deployment alongside the existing development instructions.
- [x] An ADR under `docs/decisions/` records the production deployment model (Compose plus Caddy plus systemd on a single host) and why Kubernetes and published registry images remain deferred.
- [x] Story 7.8's reconciliation claim is amended to note this drift and its resolution rather than left as-is.

**Implementation report:** The README, setup guide, known limitations, and deferred-decision list now agree that Strife ships a single-host, LAN-only Compose deployment with Caddy and systemd, while TLS productization, Kubernetes, native packages, and published registry images remain open. ADR 0011 records that boundary, the migration and rollback policy, and the reasons for deferring orchestration expansion. Story 7.8 now explicitly records the later documentation drift and this Epic 9 reconciliation.

---
