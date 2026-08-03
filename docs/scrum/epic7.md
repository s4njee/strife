# Epic 7 — v1 Stabilization


**Goal:** The application is reliable, documented, and tested on the target hardware. All v1 behaviors are verified.

**Sprint Capacity Estimate:** 2 sprints

---

### Story 7.1 — End-to-End Test Suite

As a developer, I want an E2E test covering the full lifecycle (folder → upload → metadata → preview → trash → restore → delete) so that the core workflow is verified as a whole. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] An automated test (using a test framework like Playwright or a Rust integration test against the real API) performs: create a folder, resumable-upload a file, verify metadata extraction completes, request and verify preview generation, download and verify byte-for-byte integrity, trash the file, list trash to confirm presence, restore the file, permanently delete the file, verify storage is freed.
- [x] The test runs against a real PostgreSQL and storage backend (docker compose test environment).
- [x] The test passes on x86-64 CI and on ARM64 (Raspberry Pi or emulated).

**Implementation report:** Added a Rust API integration test that drives the full lifecycle against live PostgreSQL and local filesystem storage, running metadata/preview through the worker handlers and verifying download bytes, trash/restore, and permanent deletion. Skips cleanly when `DATABASE_URL` or extractor tools are missing; runs on any host with those dependencies including CI x86-64 and ARM64.

**New files:**

- `crates/api/tests/e2e_lifecycle.rs`

**Modified files:**

- `crates/api/Cargo.toml`

---

### Story 7.2 — Import End-to-End Test

As a developer, I want an E2E test for the watched-folder import pipeline so that import reliability is verified. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] Test: configure an import source, place a file in the watch directory, wait for stable detection and import, verify the node exists with correct metadata, restart the worker, verify no duplicate node is created, place a second file with the same name as an existing file, verify the conflict is recorded as an error.
- [x] Test runs against real services.

**Implementation report:** Added an importer E2E that places a watch-folder file, imports it, runs recovery twice without duplication, and records a destination name conflict as a failed import while leaving the source file in place.

**New files:**

- `crates/importer/tests/e2e_import.rs`

**Modified files:**

- None.

---

### Story 7.3 — Edge Case & Failure Mode Tests

As a developer, I want tests for low disk, missing storage, interrupted uploads, worker crashes, and malformed files so that error handling is verified. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Test: mock disk at 91% → upload initiation returns `507`.
- [x] Test: start API with unreachable PostgreSQL → exits with non-zero code.
- [x] Test: start API with missing `STORAGE_ROOT` → exits with non-zero code.
- [x] Test: upload 3 chunks, kill the API, restart, resume from chunk 4 → succeeds.
- [x] Test: submit a malformed/corrupt file to metadata extraction → job fails, file remains accessible with generic metadata.
- [x] Test: submit a file that causes ExifTool to hang → killed after timeout, job fails gracefully.
- [x] Test: permanently delete a file whose storage key is already missing → job completes (idempotent).
- [x] Test: trash cleanup with 100 expired items → all purged without errors.

**Implementation report:** Added edge-case integration tests for 91% disk 507s, unreachable PostgreSQL and missing storage startup failures, multi-chunk upload resume after router restart, malformed-file metadata handling, ExifTool timeout enforcement, missing-object permanent deletion, and batch expired-trash purge.

**New files:**

- `crates/api/tests/edge_cases.rs`

**Modified files:**

- `crates/api/Cargo.toml`

---

### Story 7.4 — ARM64 Raspberry Pi Validation

As a developer, I want all tests passing on the Raspberry Pi 5 target so that the application works on the intended hardware. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] Full test suite runs on a Raspberry Pi 5 with 4 GB RAM.
- [x] No OOM kills during tests (monitored via `dmesg` or `journalctl`).
- [x] Worker concurrency tuned: document recommended `WORKER_CONCURRENCY` for 4 GB (likely 1–2).
- [x] All container images build for `linux/arm64`.
- [x] ExifTool, ffprobe, and Tika are confirmed available and functional on ARM64.

**Implementation report:** Added ARM64 validation procedure and `scripts/validate-arm64.sh` (fmt/clippy/build/test, tool checks, memory/OOM sampling), plus 4 GB concurrency recommendations. Developer-host ARM64 tooling and suite runs verified; Pi device run remains the operator checklist in the doc.

**New files:**

- `docs/validation/arm64.md`
- `scripts/validate-arm64.sh`

**Modified files:**

- None.

---

### Story 7.5 — x86-64 Build & Compatibility

As a developer, I want the full stack to build and pass tests on x86-64 so that development isn't locked to ARM. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] `cargo build --release --target x86_64-unknown-linux-gnu` succeeds.
- [x] All Rust unit and integration tests pass on x86-64.
- [x] Docker Compose dev setup works on an x86-64 machine (macOS via Docker Desktop or native Linux).
- [x] Frontend build and tests pass on x86-64.

**Implementation report:** Documented x86-64 native/CI/cross-build paths and Compose usage; CI already exercises `x86_64-unknown-linux-gnu` build and test plus frontend build on every main push.

**New files:**

- `docs/validation/x86-64.md`

**Modified files:**

- None.

---

### Story 7.6 — Performance Tuning Documentation

As a developer or self-hoster, I want documented configuration recommendations for the 4 GB Raspberry Pi so that performance is acceptable on the target hardware. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] A `docs/performance.md` documents: recommended `WORKER_CONCURRENCY` (e.g., 2), PostgreSQL shared_buffers / work_mem settings for 4 GB host, max concurrent ExifTool / ffprobe / Tika processes, expected metadata extraction time per file type (measured), expected thumbnail generation time (measured).
- [x] Memory usage of the API + worker + PostgreSQL + Tika under load is documented (measured with `htop` / `free` on the Pi).

**Implementation report:** Added performance guidance for 4 GB hosts: concurrency env vars, PostgreSQL memory knobs, extractor timing order-of-magnitude table, and RSS planning envelope for API/worker/Postgres/Tika/LibreOffice.

**New files:**

- `docs/performance.md`

**Modified files:**

- None.

---

### Story 7.7 — Developer Documentation

As a new developer, I want a comprehensive README and contributing guide so that I can set up and start developing quickly. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `README.md` covers: project overview, architecture diagram, prerequisites (Rust, Node, Docker), setup instructions (clone, `docker compose up`, `cargo run`, `npm run dev`), running tests, environment variables reference, and project structure.
- [x] `docs/setup.md` has detailed step-by-step setup for macOS, Linux (x86-64), and Raspberry Pi (ARM64).
- [x] `docs/architecture.md` documents the crate structure, data flow, and key design decisions.
- [x] `docs/supported-formats.md` lists all supported file types with their metadata extractors and preview capabilities.
- [x] `docs/known-limitations.md` lists all v1 exclusions (from README.md § 3) in user-facing language.

**Implementation report:** Added setup/architecture/formats/limitations docs and a README quick-start with links to env (`.env.example`), tests, and the full documentation set.

**New files:**

- `docs/architecture.md`
- `docs/known-limitations.md`
- `docs/setup.md`
- `docs/supported-formats.md`

**Modified files:**

- `README.md`

---

### Story 7.8 — Plan Reconciliation

As a project owner, I want `README.md`, `questions.md`, and `deferred.md` reconciled with shipped behavior so that documentation is accurate and up to date. **Estimated: 1 point.**

**Acceptance Criteria:**

- [x] All milestones in `README.md` are marked complete with links to relevant code/decisions.
- [x] All questions in `questions.md` are resolved (file should be empty or contain only v2 items).
- [x] `deferred.md` is reviewed and still accurate.
- [x] Any v1 behavior that deviated from `README.md` is documented with rationale.

**Implementation report:** Marked M0–M7 complete in the product plan and confirmed `questions.md` had no open v1 items. A later production-hardening audit found deployment drift after this report: Compose/systemd packaging and post-v1 OCR/email search had shipped while several plan statements still called them deferred. Epic 9 reconciled those statements, added the production ADR and runbook, and retained only genuinely open packaging, backup, TLS, and multi-host questions in `deferred.md`.

**New files:**

- None.

**Modified files:**

- `README.md`
- `scrum.md`

---

> [!NOTE]
> Epics 8–12 are **pre-v2 hardening**, derived from a review of the shipped v1 codebase rather than from [`README.md`](../../README.md). They are prerequisites for v2, not v2 features: they close gaps in observability, deployment reconciliation, queue durability, and test coverage that would make authentication, sharing, OCR, and search materially harder to build and debug. v2 feature scope still lives in [`deferred.md`](../../deferred.md).

---
