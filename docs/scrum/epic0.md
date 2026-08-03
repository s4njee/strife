# Epic 0 — Foundations & Scaffold


**Goal:** Runnable dev environment on ARM64 (and cross-compiling for x86-64) with all services healthy and the frontend connected to the API.

**Sprint Capacity Estimate:** 1–2 sprints

---

### Story 0.1 — Resolve Open Foundation Questions

As a developer, I want all Milestone 0 questions in [`questions.md`](../../questions.md) answered and recorded as Architecture Decision Records so that downstream stories have clear constraints and no implementation blocks. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] Each M0 question in `questions.md` (host OS, filesystem, ZFS vs MinIO, managed paths under `/mnt/ext`) has a written decision in `docs/decisions/` with context, decision, alternatives, consequences, and date.
- [x] `README.md` is updated to reference each decision.
- [x] The resolved questions are removed from `questions.md`.

**Implementation report:** Recorded the Gentoo/ARM64 host, direct ZFS backend, and `/mnt/ext/strife` layout as accepted architecture decisions. Reconciled the README and active questions with those decisions.

**New files:**

- `docs/decisions/0001-primary-host-platform.md`
- `docs/decisions/0002-zfs-storage-backend.md`
- `docs/decisions/0003-managed-storage-layout.md`

**Modified files:**

- `README.md`
- `questions.md`

---

### Story 0.2 — Create Rust Workspace

As a developer, I want a Cargo workspace with the planned crate structure so that teams can work on independent crates in parallel. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] Root `Cargo.toml` declares a workspace with members: `crates/api`, `crates/worker`, `crates/domain`, `crates/storage`, `crates/db`, `crates/media`, `crates/importer`.
- [x] Each crate compiles (`cargo check`) as an empty skeleton with appropriate `lib.rs` or `main.rs`.
- [x] `crates/api` and `crates/worker` are binary crates; the rest are library crates.
- [x] `cargo fmt --check` and `cargo clippy` pass with zero warnings.
- [x] A `.cargo/config.toml` is present for cross-compilation targets if needed (ARM64 + x86-64).
- [x] `cargo build --target aarch64-unknown-linux-gnu` succeeds (or documents the exact cross-compile setup).
- [x] `cargo build --target x86_64-unknown-linux-gnu` succeeds.

**Implementation report:** Created the seven-crate Rust 2024 workspace with strict shared lints and passing native format, check, Clippy, and build validation. Added Cargo aliases and exact native/containerized Linux cross-build instructions for ARM64 and x86-64.

**New files:**

- `.cargo/config.toml`
- `.gitignore`
- `Cargo.lock`
- `Cargo.toml`
- `crates/api/Cargo.toml`
- `crates/api/src/main.rs`
- `crates/db/Cargo.toml`
- `crates/db/src/lib.rs`
- `crates/domain/Cargo.toml`
- `crates/domain/src/lib.rs`
- `crates/importer/Cargo.toml`
- `crates/importer/src/lib.rs`
- `crates/media/Cargo.toml`
- `crates/media/src/lib.rs`
- `crates/storage/Cargo.toml`
- `crates/storage/src/lib.rs`
- `crates/worker/Cargo.toml`
- `crates/worker/src/main.rs`
- `docs/development/cross-compilation.md`

**Modified files:**

- None.

---

### Story 0.3 — Create SolidJS Frontend App

As a developer, I want a SolidJS + TypeScript + Vite project under `apps/web/` so that UI development can begin with a modern, type-safe toolchain. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] `apps/web/` contains a Vite + SolidJS + TypeScript project scaffolded with `degit` or `create vite`.
- [x] `npm run dev` starts the dev server and renders a placeholder page.
- [x] `npm run build` produces a production bundle without errors.
- [x] ESLint and Prettier are configured and pass on the initial codebase.
- [x] TypeScript `strict` mode is enabled in `tsconfig.json`.
- [x] All fonts and assets are locally bundled — **zero** references to external CDNs.

**Implementation report:** Scaffolded a strict SolidJS/TypeScript/Vite application with a Strife placeholder, ESLint, and Prettier. The production build, lint, formatting check, runtime dependency audit, and a live development-server smoke test all pass without external runtime assets.

**New files:**

- `apps/web/.gitignore`
- `apps/web/.prettierignore`
- `apps/web/.prettierrc.json`
- `apps/web/README.md`
- `apps/web/eslint.config.js`
- `apps/web/index.html`
- `apps/web/package-lock.json`
- `apps/web/package.json`
- `apps/web/public/favicon.svg`
- `apps/web/public/icons.svg`
- `apps/web/src/App.css`
- `apps/web/src/App.tsx`
- `apps/web/src/assets/hero.png`
- `apps/web/src/assets/solid.svg`
- `apps/web/src/assets/vite.svg`
- `apps/web/src/index.css`
- `apps/web/src/index.tsx`
- `apps/web/tsconfig.app.json`
- `apps/web/tsconfig.json`
- `apps/web/tsconfig.node.json`
- `apps/web/vite.config.ts`

**Modified files:**

- None.

---

### Story 0.4 — Development Docker Compose

As a developer, I want a `docker-compose.dev.yml` that starts PostgreSQL and Apache Tika so that I can develop locally without manual service setup. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `docker compose -f docker-compose.dev.yml up` starts PostgreSQL 16+ and Apache Tika.
- [x] PostgreSQL is accessible on `localhost:5432` with a dev database, user, and password configured via `.env.example`.
- [x] Tika is accessible on `localhost:9998`.
- [x] A `volumes` section persists PostgreSQL data across restarts.
- [x] Images used are available for both `linux/arm64` and `linux/amd64`.
- [x] `.env.example` documents every required environment variable.
- [x] `docker compose down -v` cleanly removes volumes for a fresh start.

**Implementation report:** Added digest-pinned PostgreSQL 17.10 and Apache Tika 3.2.3 development services with documented configuration and a persistent database volume. Verified both image indexes contain ARM64 and x86-64 builds, both services respond, data survives a Compose restart, and `down -v` removes the development volume.

**New files:**

- `.env.example`
- `docker-compose.dev.yml`

**Modified files:**

- `.gitignore`

---

### Story 0.5 — Database Migrations Framework

As a developer, I want SQLx migrations set up in `crates/db` with a baseline schema so that schema changes are versioned, repeatable, and checked at compile time. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] `crates/db/migrations/` contains a `00000000000000_init.sql` (or similar) that creates the database extensions (e.g., `uuid-ossp` or `pgcrypto`).
- [x] `cargo sqlx migrate run` applies migrations to the dev database.
- [x] `cargo sqlx migrate revert` rolls back the latest migration.
- [x] A `DATABASE_URL` env var is the sole connection config, documented in `.env.example`.
- [x] SQLx offline mode (`sqlx-data.json` / `cargo sqlx prepare`) is set up so CI builds work without a live database.

**Implementation report:** Added reversible embedded SQLx migrations for `pgcrypto`, a compile-time checked database ping, and checked-in offline query metadata. Verified migrate, extension creation, revert, reapply, cache generation, and offline check/Clippy with the database removed.

**New files:**

- `.sqlx/query-c48c47d412f6c66356d396d189e48643e4f0247a947c494c3d150c1d0e1cab63.json`
- `crates/db/migrations/0001_init.down.sql`
- `crates/db/migrations/0001_init.up.sql`
- `docs/development/database.md`

**Modified files:**

- `Cargo.lock`
- `Cargo.toml`
- `crates/db/Cargo.toml`
- `crates/db/src/lib.rs`

---

### Story 0.6 — API Configuration & Startup

As a developer, I want the `crates/api` binary to read config from env vars, connect to PostgreSQL, and bind to a configurable port so that the API can start up and fail fast on missing dependencies. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `crates/api` reads `DATABASE_URL`, `STORAGE_ROOT`, `LISTEN_ADDR`, and `TIKA_URL` from environment variables.
- [x] On startup, the API: connects to PostgreSQL, verifies `STORAGE_ROOT` exists and is writable, and logs a structured JSON startup message.
- [x] If PostgreSQL is unreachable, the API exits with a non-zero code and a clear error message within 5 seconds.
- [x] If `STORAGE_ROOT` is missing or unwritable, the API exits with a non-zero code and a clear error message.
- [x] Structured JSON logging is used for all log output (e.g., `tracing` + `tracing-subscriber` with JSON formatter).
- [x] The binary does **not** hard-code any paths under `/mnt/ext`; all paths are configurable.

**Implementation report:** Implemented validated environment configuration, writable-storage probing, five-second PostgreSQL startup timeout, automatic migrations, configurable binding, and structured JSON startup logs. Unit and process-level checks cover valid configuration helpers, missing/invalid storage, unreachable PostgreSQL, and successful startup with no hard-coded deployment paths.

**New files:**

- `crates/api/src/config.rs`
- `crates/api/src/lib.rs`

**Modified files:**

- `Cargo.lock`
- `Cargo.toml`
- `crates/api/Cargo.toml`
- `crates/api/src/main.rs`

---

### Story 0.7 — Health & Readiness Endpoints

As a developer or ops user, I want `GET /health` and `GET /ready` endpoints so that I can verify all dependencies are up before using the app. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] `GET /health` returns `200 OK` with `{"status": "ok"}` if the API process is running.
- [x] `GET /ready` returns `200 OK` with a JSON body including: `postgres: "ok" | "error"`, `storage: "ok" | "error"`, `tika: "ok" | "error"`, `disk_usage_percent: number`.
- [x] `GET /ready` returns `503 Service Unavailable` if any dependency check fails.
- [x] Each dependency check has a timeout (≤ 2s) so `/ready` doesn't hang.
- [x] Tests cover the healthy and degraded paths.

**Implementation report:** Added liveness and dependency-aware readiness endpoints with concurrent two-second PostgreSQL, storage/disk, and Tika checks. Unit tests cover exact health output, healthy/degraded readiness, and timeout behavior; a live stack check confirmed `200` when healthy and `503` after Tika stops.

**New files:**

- `crates/api/src/health.rs`

**Modified files:**

- `Cargo.lock`
- `Cargo.toml`
- `crates/api/Cargo.toml`
- `crates/api/src/lib.rs`

---

### Story 0.8 — Frontend ↔ API Connectivity

As a developer, I want the SolidJS app to call the API's health endpoint and display the result so that end-to-end connectivity is proven before building real features. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] A typed API client module exists at `apps/web/src/api/client.ts` using `fetch`.
- [x] The Vite dev server proxies `/api/*` to the Axum API (configured in `vite.config.ts`).
- [x] The placeholder page calls `GET /api/ready` on mount and displays the connection status.
- [x] If the API is unreachable, a user-friendly error is shown (not a raw exception).
- [x] The API client is typed: request/response types are defined in a shared `types.ts`.

**Implementation report:** Added a runtime-validating typed fetch client and a SolidJS connection card with connecting, connected, degraded, and friendly unreachable states. Verified lint/build and an end-to-end Vite `/api/ready` rewrite against the live Axum/PostgreSQL/Tika stack.

**New files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`

**Modified files:**

- `apps/web/src/App.css`
- `apps/web/src/App.tsx`
- `apps/web/vite.config.ts`

---

### Story 0.9 — Design Tokens & Theme Foundation

As a developer, I want CSS custom properties for colors, spacing, typography, and radii in dark and light themes so that all future components use consistent, theme-aware styling. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] A `tokens.css` (or equivalent) defines CSS custom properties under `:root` (light) and `[data-theme="dark"]` (dark).
- [x] Dark theme uses true-black (`#000`) background per plan spec.
- [x] Token categories include: surface colors, text colors, border colors, accent/brand colors, spacing scale, font sizes, font weights, border radii, shadows.
- [x] Fonts are locally hosted (downloaded into `apps/web/public/fonts/`) with `@font-face` declarations — no Google Fonts CDN links.
- [x] A `<ThemeProvider>` or `data-theme` toggle mechanism exists and persists the user's choice to `localStorage`.
- [x] A dev-only "theme preview" page or component renders sample swatches for visual verification.

**Implementation report:** Added complete light/dark semantic tokens, locally hosted licensed Inter and JetBrains Mono variable fonts, persistent theme context/toggle, and a development-only swatch preview. Browser verification confirmed true-black dark mode, seven rendered swatches, both local fonts loaded, light-mode switching, and persistence across reload.

**New files:**

- `apps/web/public/fonts/InterVariable.woff2`
- `apps/web/public/fonts/JetBrainsMono-Variable.woff2`
- `apps/web/public/fonts/licenses/Inter-LICENSE.txt`
- `apps/web/public/fonts/licenses/JetBrainsMono-OFL.txt`
- `apps/web/src/components/ThemePreview.css`
- `apps/web/src/components/ThemePreview.tsx`
- `apps/web/src/styles/tokens.css`
- `apps/web/src/theme/ThemeProvider.tsx`

**Modified files:**

- `apps/web/src/App.css`
- `apps/web/src/App.tsx`
- `apps/web/src/index.css`
- `apps/web/src/index.tsx`

---

### Story 0.10 — CI Skeleton & Linting

As a developer, I want a CI configuration that runs formatting, linting, and build checks so that regressions are caught before merge. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] A CI config file (GitHub Actions `.github/workflows/ci.yml` or equivalent) is present.
- [x] CI runs: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo build`, `cargo test`, `npm run lint`, `npm run build` (in `apps/web/`).
- [x] CI runs on both `push` and `pull_request` to `main`.
- [x] CI includes a matrix for `x86_64` (and optionally `aarch64` if runner is available).
- [x] A `Makefile`, `justfile`, or equivalent defines `check`, `lint`, `build`, `test` targets for local dev.

**Implementation report:** Added a least-privilege GitHub Actions workflow for main pushes and pull requests with an explicit x86-64 matrix, offline SQLx Rust checks, and frontend install/lint/format/build steps. Added local Make targets for installation, formatting, linting, build, test, aggregate checks, services, API, and web; `make check` passes end to end.

**New files:**

- `.github/workflows/ci.yml`
- `Makefile`

**Modified files:**

- None.

---
