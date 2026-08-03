# Epic 9 — Production Deployment & Process Lifecycle


**Goal:** The production deployment that already exists in the working tree is committed, documented, and survives a restart without severing in-flight work.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 9.1 — Commit Production Deployment Assets

As a project owner, I want the production deployment files committed so that the deployed configuration is version-controlled and reproducible. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] `docker-compose.prod.yml`, `deploy/docker/backend.Dockerfile`, `deploy/docker/web.Dockerfile`, `deploy/docker/Caddyfile`, `deploy/orion/strife.service`, `deploy/orion/README.md`, `.dockerignore`, and `scripts/import-icloud.sh` are committed.
- [ ] The pending `SUM(...)::BIGINT` fix in `crates/api/src/storage_usage.rs` is committed together with the regression test from Story 8.5.
- [ ] `.env.example` documents every variable the production stack requires, including `POSTGRES_PASSWORD`, `STRIFE_IMAGE_TAG`, and `STRIFE_REVISION`.
- [ ] Host secret files (`/etc/strife/postgres.env`, `/etc/strife/revision.env`) are documented but not committed, and `.gitignore` prevents accidental inclusion.
- [ ] `deploy/orion/README.md` records the host layout (`/srv/strife/storage`, `/srv/strife/postgres`, `/srv/strife/import`, `/opt/strife`) and the install, upgrade, and rollback procedures.
- [ ] A CI job builds both deployment images so a broken Dockerfile fails the build.

---

### Story 9.2 — API Graceful Shutdown

As an operator, I want the API to drain in-flight requests on SIGTERM so that restarts do not sever active uploads and downloads. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `axum::serve(...)` in `crates/api/src/lib.rs` uses `.with_graceful_shutdown(...)` with SIGTERM and Ctrl-C handling matching the pattern already implemented in `crates/worker/src/lib.rs`.
- [ ] The background task spawned by `spawn_upload_cleanup` observes the same shutdown signal and exits cleanly rather than being aborted mid-sweep.
- [ ] In-flight chunk uploads and range downloads complete before the process exits, subject to a bounded drain timeout.
- [ ] The `api` service in `docker-compose.prod.yml` sets a `stop_grace_period` consistent with that drain timeout; today only `worker` sets one (`10m`) and `api` falls back to the 10-second default.
- [ ] A shutdown log line records how many requests were drained.
- [ ] Test: a request in flight when SIGTERM is delivered completes with a success status.
- [ ] Test: restarting the stack during an active multi-chunk upload leaves the session resumable rather than failed.

---

### Story 9.3 — ARM64 Build & Test in CI

As a developer, I want CI to build and test the deployment architecture so that ARM64 regressions are caught before deployment. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] The CI matrix in `.github/workflows/ci.yml` includes `aarch64-unknown-linux-gnu` alongside `x86_64-unknown-linux-gnu`.
- [ ] The ARM64 job runs clippy, build, and the test suite under the same `-D warnings` gate.
- [ ] Container images are built for `linux/arm64` in CI, matching the claim already made in Story 7.4.
- [ ] Checks in `scripts/validate-arm64.sh` that CI now covers are removed or marked CI-covered, leaving only genuinely device-specific steps (OOM sampling, tool availability on the Pi).
- [ ] Job runtime is documented; if emulation is too slow for every push, the ARM64 job runs on merge to `main` and the split is recorded in `docs/development`.

---

### Story 9.4 — Documentation Reconciliation with Shipped Deployment

As a project owner, I want documentation to match what is actually deployed so that the plan is trustworthy going into v2. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] `docs/known-limitations.md` no longer states "Development-oriented Docker Compose; production packaging deferred"; it describes the shipped production stack and its real remaining limits (no TLS, single host, LAN-only).
- [ ] `README.md` is corrected where it states packaged deployment is a v2 concern (the deployment-direction row and the v1 non-goals section).
- [ ] `deferred.md` removes or rewrites the now-answered question "What should a production-ready Docker Compose bundle include?" and retains the genuinely open ones (TLS, Kubernetes, native binaries, published images).
- [ ] `docs/setup.md` documents production deployment alongside the existing development instructions.
- [ ] An ADR under `docs/decisions/` records the production deployment model (Compose plus Caddy plus systemd on a single host) and why Kubernetes and published registry images remain deferred.
- [ ] Story 7.8's reconciliation claim is amended to note this drift and its resolution rather than left as-is.

---
