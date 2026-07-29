# Strife v1 — Scrum Epics & Stories

> Derived from [`README.md`](README.md). Each **Epic** maps to a milestone. Stories are ordered by dependency within each epic. Point estimates use a Fibonacci scale (1, 2, 3, 5, 8, 13). Acceptance criteria are written so a mid-level dev can implement and self-verify without ambiguity.

---

## Epic 0 — Foundations & Scaffold

**Goal:** Runnable dev environment on ARM64 (and cross-compiling for x86-64) with all services healthy and the frontend connected to the API.

**Sprint Capacity Estimate:** 1–2 sprints

---

### Story 0.1 — Resolve Open Foundation Questions

As a developer, I want all Milestone 0 questions in [`questions.md`](questions.md) answered and recorded as Architecture Decision Records so that downstream stories have clear constraints and no implementation blocks. **Estimated: 3 points.**

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

## Epic 1 — Persist & Browse Folders

**Goal:** Users can create, rename, move, and navigate a persistent folder hierarchy through the UI. Both themes are functional.

**Sprint Capacity Estimate:** 2–3 sprints

---

### Story 1.1 — Hierarchy Schema & Root Node

As a developer, I want the `nodes` table with a root node automatically created so that all folder/file operations have a rooted tree to work with. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] Migration creates a `nodes` table with columns: `id` (UUID PK), `parent_id` (nullable FK to self), `name` (text, not null), `kind` (enum: `folder` | `file`), `lifecycle_state` (enum: `active` | `trashed` | `deleted`), `source_created_at` (timestamptz, nullable), `source_modified_at` (timestamptz, nullable), `created_at` (timestamptz, not null, default now), `updated_at` (timestamptz, not null, default now).
- [x] A unique constraint enforces case-sensitive sibling uniqueness: `UNIQUE (parent_id, name) WHERE lifecycle_state = 'active'`.
- [x] A root node (e.g., `parent_id IS NULL`, `name = 'root'`, `kind = 'folder'`) is inserted by the migration or by application startup idempotently.
- [x] A CHECK or trigger prevents a node from being its own parent.
- [x] `crates/db` exposes typed query functions: `get_node_by_id`, `list_children`.
- [x] Integration tests verify the root exists after migration and that the unique constraint rejects duplicate sibling names.

**Implementation report:** Added PostgreSQL node enums, the rooted hierarchy table, active-sibling uniqueness, self-parent protection, and an idempotent stable root record. Added typed node reads plus integration coverage against PostgreSQL; CI now supplies PostgreSQL so those tests exercise the real migration and constraint.

**New files:**

- `crates/db/migrations/0002_nodes.down.sql`
- `crates/db/migrations/0002_nodes.up.sql`
- `crates/db/tests/hierarchy.rs`

**Modified files:**

- `.github/workflows/ci.yml`
- `Cargo.lock`
- `Cargo.toml`
- `crates/db/Cargo.toml`
- `crates/db/src/lib.rs`

---

### Story 1.2 — Folder CRUD API

As an API consumer, I want endpoints to list, create, rename, and move folders so that the folder hierarchy can be managed programmatically. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `GET /api/folders/:id/children` returns paginated children (cursor-based) sorted by name by default. Response includes `id`, `name`, `kind`, `created_at`, `updated_at`, and a `next_cursor` if more results exist.
- [x] `POST /api/folders` with `{ "parent_id": UUID, "name": string }` creates a folder. Returns `201` with the created folder. Returns `409 Conflict` with details if a sibling with the same name exists.
- [x] `PATCH /api/folders/:id` with `{ "name": string }` renames a folder. Returns `200` with the updated folder. Returns `409` on name conflict.
- [x] `PATCH /api/folders/:id` with `{ "parent_id": UUID }` moves a folder. Returns `200`. Returns `409` on name conflict at the destination. Returns `400` if the move would create a cycle (moving a folder into its own descendant).
- [x] All mutations are wrapped in database transactions.
- [x] A cycle-detection query (recursive CTE) prevents moving a folder under itself.
- [x] API tests cover: create, rename, move, conflict rejection, cycle rejection, and listing with pagination.

**Implementation report:** Added cursor-paginated folder listing plus transactional create, rename, and move endpoints under `/api`, with structured conflict/not-found errors and recursive-CTE cycle rejection. PostgreSQL-backed API tests exercise the complete mutation workflow, name conflicts, cycles, and multi-page name ordering while legacy health paths remain available.

**New files:**

- `crates/api/src/folders.rs`
- `crates/api/tests/folders_api.rs`

**Modified files:**

- `Cargo.lock`
- `Cargo.toml`
- `apps/web/vite.config.ts`
- `crates/api/Cargo.toml`
- `crates/api/src/health.rs`
- `crates/api/src/lib.rs`
- `crates/db/Cargo.toml`
- `crates/db/src/lib.rs`

---

### Story 1.3 — Domain Layer — Folder Rules

As a developer, I want folder hierarchy rules encapsulated in `crates/domain` so that business logic is testable independent of HTTP or DB. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `crates/domain` defines types: `NodeId`, `NodeKind`, `LifecycleState`, `Node`, and a `FolderTree` or equivalent service trait.
- [x] Pure functions or methods validate: name is non-empty, move target is not a descendant, sibling name is unique (given a list of existing siblings).
- [x] Error types clearly distinguish: `NameConflict`, `CycleDetected`, `NotFound`, `InvalidName`.
- [x] Unit tests cover all validation rules without touching a database.

**Implementation report:** Added database- and HTTP-independent node types, a `FolderTree` boundary, explicit folder error variants, and pure validation for names, active sibling uniqueness, and move cycles. Database mutations and API validation now consume the domain rules, with unit tests covering every rule without external services.

**New files:**

- None.

**Modified files:**

- `Cargo.lock`
- `crates/api/Cargo.toml`
- `crates/api/src/folders.rs`
- `crates/db/Cargo.toml`
- `crates/db/src/lib.rs`
- `crates/domain/Cargo.toml`
- `crates/domain/src/lib.rs`

---

### Story 1.4 — Application Shell & Sidebar

As a user, I want a sidebar showing "All Files", "Favorites", and "Trash" navigation items so that I can navigate between main views. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] A `<Sidebar>` component renders navigation links: "All Files" (root), "Favorites", "Trash".
- [x] The active item is visually highlighted based on the current route.
- [x] Sidebar includes a storage usage summary area (placeholder value for now — real data comes in Epic 6).
- [x] Sidebar renders correctly in both dark and light themes.
- [x] The sidebar is a fixed-width panel on the left; the main content area fills the remaining width.
- [x] SolidJS Router is configured with routes: `/` (root folder), `/folder/:id`, `/favorites`, `/trash`.

**Implementation report:** Replaced the foundation screen with a routed SolidJS application shell, fixed-width navigation sidebar, active All Files/Favorites/Trash states, storage placeholder, and flexible workspace views. Added the current Solid Router and browser-verified route changes, the 240px/content layout, and sidebar colors in both light and true-black themes.

**New files:**

- `apps/web/src/components/Sidebar.css`
- `apps/web/src/components/Sidebar.tsx`
- `apps/web/src/views/WorkspaceView.tsx`

**Modified files:**

- `apps/web/package-lock.json`
- `apps/web/package.json`
- `apps/web/src/App.css`
- `apps/web/src/App.tsx`
- `apps/web/src/index.css`
- `apps/web/src/index.tsx`

---

### Story 1.5 — Breadcrumb Navigation

As a user, I want a breadcrumb trail showing my current path in the folder hierarchy so that I know where I am and can navigate to parent folders. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] `GET /api/folders/:id/ancestors` returns an ordered array of `{ id, name }` from root to the requested folder.
- [x] A `<Breadcrumb>` component renders each ancestor as a clickable link.
- [x] Clicking an ancestor navigates to that folder.
- [x] The current folder name is displayed but not clickable.
- [x] Breadcrumbs truncate gracefully if the path is very deep (e.g., ellipsis after 5 levels with a hover/expand).

**Implementation report:** Added a recursive ancestor query and `/api/folders/:id/ancestors` endpoint, then connected it to a routed breadcrumb that keeps the current folder non-interactive. Browser verification against a temporary six-level PostgreSQL hierarchy confirmed root-to-current links, five-part truncation, and full-path expansion.

**New files:**

- `apps/web/src/components/Breadcrumb.css`
- `apps/web/src/components/Breadcrumb.tsx`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/views/WorkspaceView.tsx`
- `crates/api/src/folders.rs`
- `crates/api/tests/folders_api.rs`
- `crates/db/src/lib.rs`

---

### Story 1.6 — File Table Component

As a user, I want a dense table displaying folder contents with columns for name, kind, size, and date so that I can see what's in a folder at a glance. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] A `<FileTable>` component renders children of the current folder in a table.
- [x] Columns: checkbox (selection), icon (folder/file), Name, Kind, Size (formatted: KB/MB/GB), Date Modified.
- [x] Folders are listed before files by default.
- [x] Double-clicking a folder row navigates into it.
- [x] Empty folder shows a centered empty state message: "This folder is empty".
- [x] Loading state shows a skeleton/shimmer animation.
- [x] Error state shows a retry button with an error message.
- [x] Rows have hover highlighting and alternating row shading in both themes.

**Implementation report:** Added a dense folder-contents table with checkbox/icon/name/kind/size/date columns, folder-first ordering, formatted sizes and dates, folder double-click navigation, and explicit shimmer, retryable error, and empty states. Browser checks covered loading-to-empty behavior plus representative hosted-preview folder/file rows, including a 2.5 MB file and successful routed folder opening.

**New files:**

- `apps/web/src/components/FileTable.css`
- `apps/web/src/components/FileTable.tsx`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/views/WorkspaceView.tsx`

---

### Story 1.7 — Selection Model

As a user, I want desktop file-manager style selection with single click, Shift-click, and Cmd/Ctrl-click so that I can select one or multiple items naturally. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Single click on a row selects it and deselects all others.
- [x] Cmd-click (Mac) / Ctrl-click (Windows/Linux) toggles the clicked row's selection without affecting others.
- [x] Shift-click selects a contiguous range from the last selected item (anchor) to the clicked item.
- [x] Clicking the row checkbox toggles that single row without clearing other selections.
- [x] A "Select All" checkbox in the header toggles all visible items.
- [x] Selection state is stored in a SolidJS signal/store, not in the DOM.
- [x] Selected rows have a distinct background color in both themes.
- [x] A selection count is displayed somewhere visible (e.g., status bar or toolbar) when ≥ 1 item is selected.

**Implementation report:** Added a signal-backed selection model with single replacement, Cmd/Ctrl additive toggles, anchor-based Shift ranges, independent row checkboxes, select-all and indeterminate states, selected-row theming, and a live count. Browser interaction checks confirmed every selection path and the exact selected item sets across the hosted-preview rows.

**New files:**

- None.

**Modified files:**

- `apps/web/src/components/FileTable.css`
- `apps/web/src/components/FileTable.tsx`

---

### Story 1.8 — Context Menu

As a user, I want a right-click context menu on selected items with folder operations so that I can perform actions without hunting for buttons. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] Right-clicking a row (or selected rows) opens a context menu.
- [x] Menu items for folders: "Open", "Rename", "Move to…", "Move to Trash".
- [x] Menu items are contextually correct (e.g., "Open" only for folders in this milestone; files come later).
- [x] Clicking outside the menu or pressing Escape closes it.
- [x] The menu is positioned near the cursor and adjusts to stay within the viewport.
- [x] Menu renders correctly in both themes.
- [x] If multiple items are selected and you right-click one of them, the menu applies to the entire selection.
- [x] If you right-click a non-selected item, it selects only that item and opens the menu for it.

**Implementation report:** Added a cursor-positioned, viewport-clamped context menu with folder-aware Open, Rename, Move to…, and Move to Trash actions, plus outside-click and Escape dismissal. Browser checks confirmed single and multi-folder menus, selection preservation, replacement when right-clicking an unselected file, contextually absent file actions, and an eight-pixel viewport edge clamp.

**New files:**

- `apps/web/src/components/ContextMenu.css`
- `apps/web/src/components/ContextMenu.tsx`

**Modified files:**

- `apps/web/src/components/FileTable.tsx`

---

### Story 1.9 — Create Folder Dialog

As a user, I want a dialog to create a new folder in the current directory so that I can organize my files. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] A "New Folder" button in the toolbar (or via context menu on empty space) opens a modal dialog.
- [x] Dialog has a text input for folder name, a "Create" button, and a "Cancel" button.
- [x] On submit, calls `POST /api/folders` with the current folder as parent.
- [x] On success, the new folder appears in the table without a full page refresh.
- [x] On `409 Conflict`, the dialog shows an inline error: "A folder with this name already exists".
- [x] The input is auto-focused on open. Enter submits; Escape cancels.

**Implementation report:** Added a New Folder toolbar action, accessible modal form, typed create-folder API client, current-folder refetch, and exact inline 409 handling; the static Pages preview also supports local demonstration creates. Browser tests against the real Axum/PostgreSQL stack confirmed autofocus, Escape cancellation, button and Enter submission, live table refresh, and duplicate-name feedback, then removed the temporary folders.

**New files:**

- `apps/web/src/components/CreateFolderDialog.css`
- `apps/web/src/components/CreateFolderDialog.tsx`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/views/WorkspaceView.tsx`

---

### Story 1.10 — Rename Inline or Dialog

As a user, I want to rename a folder via the context menu so that I can fix naming mistakes quickly. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] "Rename" from the context menu opens an inline edit field on the row's name cell (or a small dialog).
- [x] The current name is pre-filled and fully selected.
- [x] Pressing Enter or clicking "Save" calls `PATCH /api/folders/:id`.
- [x] On success, the row updates in place.
- [x] On `409 Conflict`, an inline error message appears.
- [x] Pressing Escape cancels the rename.

**Implementation report:** Added a context-menu rename dialog with selected current-name input, keyboard controls, typed PATCH client, in-place resource updates, and exact inline conflict handling. Browser tests against the real Axum/PostgreSQL stack confirmed Enter submission, immediate row replacement, duplicate-name feedback, and Escape cancellation.

**New files:**

- `apps/web/src/components/RenameFolderDialog.tsx`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/views/WorkspaceView.tsx`

---

### Story 1.11 — Move Folder Dialog

As a user, I want a dialog to move a folder (or selected folders) to another location so that I can reorganize my hierarchy. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] "Move to…" from the context menu opens a modal with a folder tree browser.
- [x] The tree browser loads folders lazily from `GET /api/folders/:id/children` (folders only).
- [x] The selected item(s) and their descendants are disabled/greyed out (to prevent cycles).
- [x] Clicking "Move" calls `PATCH /api/folders/:id` with the new `parent_id` for each selected item. The entire batch fails or succeeds atomically.
- [x] On success, items disappear from the current view and the table refreshes.
- [x] On conflict, the dialog shows which items conflicted and why.

**Implementation report:** Added a lazy folder-only destination tree and an atomic batch PATCH endpoint that validates all sources, cycle targets, and sibling-name conflicts in one PostgreSQL transaction. API integration and browser tests confirmed all-or-nothing behavior, disabled selected descendants, item-specific conflict reasons, and immediate removal of a successful multi-folder move from the current table.

**New files:**

- `apps/web/src/components/MoveFolderDialog.css`
- `apps/web/src/components/MoveFolderDialog.tsx`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/views/WorkspaceView.tsx`
- `crates/api/src/folders.rs`
- `crates/api/tests/folders_api.rs`
- `crates/db/src/lib.rs`

---

### Story 1.12 — Dark & Light Theme Toggle

As a user, I want a toggle button to switch between dark and light themes so that I can use the app in my preferred visual mode. **Estimated: 1 point.**

**Acceptance Criteria:**

- [x] A theme toggle button is visible in the top bar or sidebar footer.
- [x] Clicking it toggles `data-theme` between `"dark"` and `"light"` on the root element.
- [x] The chosen theme is persisted to `localStorage` and restored on page load.
- [x] Default theme is dark (true-black).
- [x] All existing components render correctly in both themes (visual spot-check).

**Implementation report:** Made theme restoration synchronous and added a pre-render bootstrap so persisted preferences apply without a flash while new users reliably receive the true-black dark default. Browser spot-checks covered the sidebar, folder table, create dialog, and move tree in both themes and confirmed light/dark persistence across reloads.

**New files:**

- None.

**Modified files:**

- `apps/web/index.html`
- `apps/web/src/theme/ThemeProvider.tsx`

---

## Epic 2 — Resumable Upload & Download

**Goal:** Files can be uploaded (resumably, with chunking), downloaded, and range-streamed. Uploads survive page reloads and service restarts.

**Sprint Capacity Estimate:** 3–4 sprints

---

### Story 2.1 — Storage Backend Abstraction

As a developer, I want a `StorageBackend` trait in `crates/storage` with implementations for the chosen backend so that file I/O is decoupled from the rest of the application. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `crates/storage` defines a `StorageBackend` trait with async methods: `put_stream(key, stream) -> Result<()>`, `get_stream(key) -> Result<impl AsyncRead>`, `get_range(key, offset, length) -> Result<impl AsyncRead>`, `delete(key) -> Result<()>`, `exists(key) -> Result<bool>`, `disk_usage() -> Result<DiskUsage>`.
- [x] A `LocalFsBackend` (or `MinioBackend`) implements the trait using the decided storage approach.
- [x] Storage keys are opaque UUIDs — display names are **never** used as file paths.
- [x] Three separate namespaces (directories or prefixes) exist: `staging/`, `originals/`, `artifacts/`.
- [x] `put_stream` writes atomically: write to a temp file, then rename (or use MinIO's multipart upload).
- [x] Integration tests verify put/get/delete round-trip and that `get_range` returns correct byte ranges.
- [x] `disk_usage()` returns total, used, and available bytes for the storage volume.

**Implementation report:** Added an object-safe asynchronous storage contract and local-filesystem implementation with strongly typed UUID keys, isolated staging/originals/artifacts namespaces, and atomic temporary-file publication. Integration tests verify full round trips, exact ranged reads, idempotent deletion, namespace creation, and consistent total/used/available capacity reporting.

**New files:**

- `crates/storage/tests/local_fs.rs`

**Modified files:**

- `Cargo.lock`
- `Cargo.toml`
- `crates/storage/Cargo.toml`
- `crates/storage/src/lib.rs`

---

### Story 2.2 — File Object Schema

As a developer, I want the `file_objects` table linked to `nodes` so that uploaded file metadata is persisted alongside the hierarchy. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] Migration creates `file_objects` with columns: `id` (UUID PK), `node_id` (FK to `nodes`, unique for finalized objects), `storage_key` (text, not null), `byte_size` (bigint, not null), `mime_type` (text), `checksum_sha256` (text), `upload_state` (enum: `staging` | `finalized`), `created_at`, `updated_at`.
- [x] A constraint ensures a finalized node has exactly one finalized `file_object`.
- [x] DB query functions: `create_file_object`, `finalize_file_object`, `get_file_object_by_node_id`.

**Implementation report:** Added the `file_objects` migration, typed staged/finalized records, and create/finalize/get database queries with nonnegative-size and finalized-node constraints. A live PostgreSQL integration test confirms staged-to-finalized transitions and database rejection of a second finalized object for the same node.

**New files:**

- `crates/db/migrations/0003_file_objects.down.sql`
- `crates/db/migrations/0003_file_objects.up.sql`
- `crates/db/tests/file_objects.rs`

**Modified files:**

- `crates/db/src/lib.rs`

---

### Story 2.3 — Upload Session Schema

As a developer, I want the `upload_sessions` table to track resumable uploads so that chunk progress is durable across reloads and restarts. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] Migration creates `upload_sessions` with columns: `id` (UUID PK), `target_folder_id` (FK to `nodes`), `display_name` (text), `expected_byte_size` (bigint, nullable), `received_bytes` (bigint, default 0), `staging_key` (text), `state` (enum: `active` | `finalizing` | `completed` | `cancelled` | `expired`), `checksum_sha256` (text, nullable), `source_created_at` (timestamptz, nullable), `source_modified_at` (timestamptz, nullable), `expires_at` (timestamptz), `created_at`, `updated_at`.
- [x] A separate `upload_chunks` table (or `received_ranges` jsonb column) tracks which byte ranges have been received.
- [x] DB query functions: `create_session`, `record_chunk`, `get_session_progress`, `finalize_session`, `cancel_session`, `list_expired_sessions`.

**Implementation report:** Added durable upload-session and chunk-range schemas with lifecycle, expiry, active-name uniqueness, completed-node linkage, and ordered byte-range tracking. Typed queries and live PostgreSQL tests cover session creation, out-of-order non-overlapping chunks, atomic byte totals, overlap rejection, progress retrieval, completion, idempotent cancellation, and expiry listing.

**New files:**

- `crates/db/migrations/0004_upload_sessions.down.sql`
- `crates/db/migrations/0004_upload_sessions.up.sql`
- `crates/db/tests/upload_sessions.rs`

**Modified files:**

- `crates/db/src/lib.rs`

---

### Story 2.4 — Upload Initiation Endpoint

As an API client, I want `POST /api/uploads` to create an upload session so that I get a session ID to send chunks to. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `POST /api/uploads` accepts `{ "folder_id": UUID, "name": string, "size": number | null, "source_created_at": string | null, "source_modified_at": string | null }`.
- [ ] Validates: folder exists and is active, no sibling name conflict among active nodes **and** other active upload sessions, disk usage is below 90% (or below 90% + declared size if size is known).
- [ ] Creates a staging storage key and an `upload_sessions` row.
- [ ] Returns `201` with `{ "session_id": UUID, "staging_key": string }`.
- [ ] Returns `409` on name conflict, `507` on disk full, `404` if folder doesn't exist.
- [ ] Session expires after a configurable TTL (default: 24 hours).

---

### Story 2.5 — Chunk Upload Endpoint

As an API client, I want `PATCH /api/uploads/:session_id` to upload a chunk with a byte range so that large files can be uploaded incrementally. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Accepts a `Content-Range` header (e.g., `bytes 0-1048575/10485760`) and the chunk body.
- [ ] Streams the chunk body to the staging file at the correct offset — does **not** buffer the entire chunk in memory.
- [ ] Updates `upload_sessions.received_bytes` and records the range in `upload_chunks`.
- [ ] Returns `200` with current progress: `{ "received_bytes": number, "expected_bytes": number | null, "complete": boolean }`.
- [ ] Rejects overlapping or duplicate ranges with `409`.
- [ ] Rejects chunks for non-active sessions with `404` or `410 Gone`.
- [ ] Incrementally computes SHA-256 checksum as chunks arrive (or on finalization).
- [ ] Handles out-of-order chunks correctly.

---

### Story 2.6 — Upload Finalization Endpoint

As an API client, I want `POST /api/uploads/:session_id/finalize` to commit the upload so that the file becomes a real node in the hierarchy. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Verifies all bytes are received (if `expected_byte_size` was set, `received_bytes` matches).
- [ ] Computes (or finalizes) the SHA-256 checksum over the staging file.
- [ ] Detects MIME type from file content bytes (using `libmagic` / `file` command), **not** from the file extension.
- [ ] In a single transaction: moves the staging file to `originals/`, creates a `nodes` row (kind = `file`), creates a finalized `file_objects` row, updates the session to `completed`, and enqueues a metadata extraction job.
- [ ] Re-checks name conflict at finalization time (another upload may have raced).
- [ ] Returns `200` with the created node.
- [ ] Returns `409` on name conflict, `400` if bytes are incomplete.
- [ ] The operation is idempotent: calling finalize on an already-completed session returns the existing node.
- [ ] Source timestamps (`source_created_at`, `source_modified_at`) from the session are preserved on the node.

---

### Story 2.7 — Upload Cancellation & Cleanup

As an API client or system operator, I want to cancel an upload and have stale sessions cleaned up automatically so that staging space is reclaimed. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `DELETE /api/uploads/:session_id` cancels an active session, marks it `cancelled`, and deletes the staging file.
- [ ] A background task (in `crates/worker` or the API process) runs periodically (e.g., every 15 minutes) to find expired sessions (`expires_at < now()`), delete their staging files, and mark them `expired`.
- [ ] Cancellation and cleanup are idempotent.
- [ ] Tests verify that a cancelled/expired session's staging file is removed from disk.

---

### Story 2.8 — Upload Progress Query

As an API client, I want `GET /api/uploads/:session_id` to check upload progress so that the UI can resume from where it left off after a reload. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] Returns `{ "session_id", "state", "display_name", "received_bytes", "expected_bytes", "received_ranges": [...], "created_at", "expires_at" }`.
- [ ] `GET /api/uploads?folder_id=:id` lists all active sessions for a folder.
- [ ] Used by the frontend to detect in-progress uploads on page load and resume them.

---

### Story 2.9 — File Download & Range Requests

As a user, I want to download a file and have video/audio seek via HTTP ranges so that I can retrieve my files and stream media. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `GET /api/files/:node_id/download` returns the original file with correct `Content-Type`, `Content-Length`, and `Content-Disposition: attachment; filename="<display_name>"` headers.
- [ ] If the request includes a `Range` header, respond with `206 Partial Content`, `Content-Range`, and the requested byte range.
- [ ] Support multi-range requests (or at minimum single-range).
- [ ] Stream the file from storage — do **not** load the entire file into memory.
- [ ] Return `404` for non-existent or trashed nodes.
- [ ] Tests verify full download, single range, and that the downloaded content matches the uploaded content byte-for-byte.

---

### Story 2.10 — Folder Upload & Hierarchy Preservation

As a user, I want to upload an entire folder and have its directory structure preserved so that I don't have to recreate my folder hierarchy manually. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] The frontend reads `webkitRelativePath` from the `File` objects when a folder is selected via `<input webkitdirectory>`.
- [ ] Before uploading, the client resolves the relative paths and issues `POST /api/folders` calls to create any missing intermediate folders.
- [ ] Each file upload session references its correct parent folder.
- [ ] If any folder creation or file upload fails due to a name conflict, the error is reported per-item; other non-conflicting items continue uploading.
- [ ] The final folder structure in Strife mirrors the original on-disk structure.
- [ ] Test: upload a folder with 3 levels of nesting and verify the hierarchy in the API.

---

### Story 2.11 — Disk Guard

As a system, I want upload initiation rejected when disk usage ≥ 90% so that the server doesn't fill up and crash. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] Before creating an upload session, check `storage.disk_usage()`.
- [ ] If usage ≥ 90%, return `507 Insufficient Storage` with `{ "error": "disk_full", "usage_percent": number }`.
- [ ] The 90% threshold is a configurable environment variable (`DISK_GUARD_PERCENT`, default 90).
- [ ] The same check runs before watched-folder imports (Epic 3).
- [ ] Test with a mock that simulates 91% usage and verify rejection.

---

### Story 2.12 — Upload UI — File Picker & Drag-Drop

As a user, I want to upload files via a file picker button and drag-and-drop so that uploading is fast and intuitive. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] An "Upload" button in the toolbar opens the native file picker (supports multi-select).
- [ ] A second option or mode allows folder selection (`webkitdirectory`).
- [ ] Dragging files/folders onto the table area shows a visual drop zone overlay.
- [ ] Dropping files initiates upload sessions for each file.
- [ ] Files are chunked client-side (default chunk size: 1 MB, configurable).
- [ ] Each chunk is uploaded via `PATCH /api/uploads/:session_id` with the correct `Content-Range`.
- [ ] Concurrent uploads are limited (e.g., max 3 simultaneous file uploads).

---

### Story 2.13 — Upload UI — Progress, Resume & Cancel

As a user, I want to see upload progress, resume after a reload, and cancel uploads so that I have full control over ongoing uploads. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] A persistent upload progress panel (bottom of the screen or a drawer) shows all active uploads with: file name, progress bar (percentage), bytes uploaded / total, estimated time remaining, and a cancel button.
- [ ] On page load, the app queries `GET /api/uploads?folder_id=...` for active sessions and resumes them automatically.
- [ ] Resuming: the app reads `received_ranges` from the session, identifies missing byte ranges, and uploads only those ranges.
- [ ] Clicking "Cancel" calls `DELETE /api/uploads/:session_id` and removes the item from the progress panel.
- [ ] When an upload completes, the file appears in the table immediately (optimistic update or refetch).
- [ ] Conflict errors are displayed inline per-file in the progress panel.
- [ ] The progress panel survives navigation between folders (it's outside the route content area).

---

### Story 2.14 — Low Disk Warning UI

As a user, I want a persistent notification when disk usage is high so that I know to free space before uploads start failing. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] The app periodically checks `GET /api/ready` (or a dedicated endpoint) for `disk_usage_percent`.
- [ ] When usage ≥ 80%, a persistent warning banner appears at the top of the content area: "Storage is almost full (X% used)".
- [ ] When usage ≥ 90%, the banner becomes an error state: "Storage is full. Uploads and imports are disabled."
- [ ] The banner is not dismissible while the condition persists.
- [ ] When usage drops below 80%, the banner disappears.

---

## Epic 3 — Watched-Folder Import

**Goal:** Files placed in a configured server-side directory are automatically discovered, validated, and imported into Strife, using the same finalization pipeline as uploads.

**Sprint Capacity Estimate:** 2 sprints

---

### Story 3.1 — Resolve Import Questions

As a developer, I want all Milestone 3 questions in [`questions.md`](questions.md) decided and recorded so that import behavior is unambiguous. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] Decisions recorded in `docs/decisions/` for: copy vs move, watch path → destination mapping, stability detection, post-import source handling, re-import/disappearance handling, conflict handling.
- [ ] `questions.md` M3 section is cleared; `README.md` updated.

---

### Story 3.2 — Import Source Schema

As a developer, I want `import_sources` and `import_entries` tables so that import configuration and per-file state are durably tracked. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `import_sources` has: `id`, `watch_path` (text, unique), `destination_folder_id` (FK), `enabled` (bool), `last_scan_at`, `created_at`, `updated_at`.
- [ ] `import_entries` has: `id`, `source_id` (FK), `source_path` (text), `source_size` (bigint), `source_modified_at`, `source_checksum` (text, nullable), `state` (enum: `discovered` | `stable` | `importing` | `imported` | `failed`), `resulting_node_id` (FK, nullable), `error_message` (text, nullable), `created_at`, `updated_at`.
- [ ] A unique constraint on `(source_id, source_path)` prevents duplicate tracking of the same file.
- [ ] DB queries: `upsert_import_entry`, `list_pending_entries`, `mark_imported`, `mark_failed`.

---

### Story 3.3 — File Discovery Scanner

As a system, I want a periodic directory scanner in `crates/importer` so that new files in the watched folder are detected. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] A scanner function walks the configured `watch_path` recursively.
- [ ] For each regular file found, it upserts an `import_entries` row with `state = discovered`, recording size and modification time.
- [ ] Symbolic links, device files, sockets, and other special files are **skipped** (logged at debug level).
- [ ] Hidden files (starting with `.`) are skipped by default (configurable).
- [ ] Directories in the watch path are recorded so hierarchy can be recreated.
- [ ] The scanner runs on a configurable interval (default: 60 seconds).
- [ ] The scanner is idempotent: re-scanning the same unchanged file does not create duplicate entries.
- [ ] Tests: create files in a temp dir, run the scanner, verify entries are created.

---

### Story 3.4 — Stability Detection

As a system, I want to only import files that have been stable (unchanged) for a configured period so that partially written files are not imported. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] After discovery, a file's size and modification time are compared to the previous scan.
- [ ] If unchanged for N consecutive scans (configurable, default: 2 scans = 2 minutes at 60s interval), the entry transitions to `state = stable`.
- [ ] If the file changes between scans, the stability counter resets.
- [ ] Only `stable` entries proceed to the import pipeline.
- [ ] Tests: simulate a file that changes between scans and verify it is not imported prematurely.

---

### Story 3.5 — Import Pipeline

As a system, I want stable files processed through the same checksum/finalization pipeline as uploads so that imports are consistent and reliable. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] For each stable entry, the importer: checks disk guard (90%), checks name conflict at the destination, streams the file into staging via `StorageBackend`, computes SHA-256 during streaming, detects MIME, and atomically finalizes (creates node, file_object, marks entry as `imported`).
- [ ] Source filesystem timestamps are preserved on the created node.
- [ ] Hierarchy is preserved: if the source is `watch_path/photos/2024/img.jpg`, create folders `photos` and `2024` under the destination before importing `img.jpg`.
- [ ] Folder creation reuses existing folders if they already exist (no conflict on pre-existing matching folder).
- [ ] On conflict (duplicate file name), the entry is marked `failed` with a clear error message; it does **not** block other imports.
- [ ] On completion, a metadata extraction job is enqueued.
- [ ] Tests: import a tree of 5 files across 3 directories; verify nodes, hierarchy, checksums, and no duplicates.

---

### Story 3.6 — Import Restart & Idempotency

As a system, I want imports to survive service restarts without duplication so that the system is reliable. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] On startup, the importer loads all `import_entries` with state `importing` and retries them.
- [ ] If a node was already created (crash after finalization but before marking `imported`), the retry detects it via the source path unique constraint and marks it `imported`.
- [ ] If staging was written but not finalized, the retry re-finalizes.
- [ ] No restart scenario creates a duplicate node for the same source file.
- [ ] Test: simulate a crash mid-import (kill the process), restart, verify exactly one node exists.

---

### Story 3.7 — Import Management API

As a user, I want API endpoints to configure and monitor watched-folder imports so that I can control imports without SSH-ing into the server. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `POST /api/import-sources` creates a new watched source: `{ "watch_path": string, "destination_folder_id": UUID }`. Validates the path exists and is readable. Returns `400` if the path overlaps managed storage.
- [ ] `GET /api/import-sources` lists all sources with their status (enabled, last scan time, entry counts by state).
- [ ] `PATCH /api/import-sources/:id` toggles `enabled`.
- [ ] `GET /api/import-sources/:id/entries?state=failed` lists entries filtered by state, with error messages.
- [ ] `POST /api/import-sources/:id/entries/:entry_id/retry` resets a failed entry to `discovered` for re-processing.

---

### Story 3.8 — Import Status UI

As a user, I want to see import progress and errors in the UI so that I know what's happening with my watched folder. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] A section in the sidebar or a dedicated page shows configured import sources.
- [ ] Each source displays: watch path, destination, enabled/disabled, last scan time, counts (discovered / importing / imported / failed).
- [ ] Failed entries are listed with their error messages and a "Retry" button.
- [ ] A toggle to enable/disable the source is available.
- [ ] The status refreshes periodically (every 30 seconds) or on user action.

---

## Epic 4 — Metadata Extraction

**Goal:** Uploaded and imported files have rich metadata extracted asynchronously via durable background jobs, without blocking ingestion.

**Sprint Capacity Estimate:** 3 sprints

---

### Story 4.1 — Resolve Metadata Questions

As a developer, I want M4 questions decided (raw retention, typed columns, format test matrix) so that metadata schema and extractor implementation are clear. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] Decisions recorded in `docs/decisions/` for: raw metadata size/retention policy, first-class typed columns, and the explicit format test matrix.
- [ ] `questions.md` M4 section is cleared.

---

### Story 4.2 — Jobs Schema & Queue

As a developer, I want a `jobs` table and a `FOR UPDATE SKIP LOCKED` job queue so that metadata and preview work is durable and retry-safe. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Migration creates `jobs` with: `id` (UUID), `job_type` (enum: `metadata_extraction` | `preview_generation` | `trash_cleanup` | `permanent_deletion`), `target_node_id` (FK), `state` (enum: `pending` | `leased` | `completed` | `failed` | `cancelled`), `priority` (int, default 0), `attempts` (int, default 0), `max_attempts` (int, default 3), `lease_owner` (text, nullable), `lease_expires_at` (timestamptz, nullable), `last_error` (text, nullable), `created_at`, `updated_at`, `completed_at`.
- [ ] `claim_job(job_type, owner)` uses `SELECT ... FOR UPDATE SKIP LOCKED` to lease the highest-priority pending job, setting `lease_owner`, `lease_expires_at` (now + configurable TTL), and incrementing `attempts`.
- [ ] `complete_job(id)` marks it `completed`.
- [ ] `fail_job(id, error)` marks it `failed` if `attempts >= max_attempts`, otherwise resets to `pending` with the error recorded.
- [ ] `release_expired_leases()` finds jobs where `lease_expires_at < now()` and resets them to `pending`.
- [ ] Enqueueing the same `(job_type, target_node_id)` when one is already `pending` is a no-op (idempotent).
- [ ] Tests: enqueue, claim, complete, fail with retry, expire lease.

---

### Story 4.3 — Worker Binary & Job Loop

As a developer, I want `crates/worker` to run a loop claiming and executing jobs so that metadata and previews are processed in the background. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `crates/worker` is a binary that connects to PostgreSQL and the storage backend.
- [ ] It runs a configurable number of concurrent job processors (default: 2, tunable for 4 GB RAM via `WORKER_CONCURRENCY`).
- [ ] Each processor loops: claim a job → execute the handler → complete/fail the job.
- [ ] If no job is available, the processor sleeps for a configurable interval (default: 5s) before polling again.
- [ ] A periodic task (every 60s) calls `release_expired_leases()`.
- [ ] Structured JSON logging with a `job_id` correlation ID on every log line during processing.
- [ ] Graceful shutdown on SIGTERM: finish current jobs, then exit.

---

### Story 4.4 — Metadata Schema

As a developer, I want `metadata_records` and `media_streams` tables so that extracted metadata is stored durably. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `metadata_records`: `id`, `node_id` (FK), `extractor_name` (text), `extractor_version` (text), `status` (enum: `pending` | `completed` | `failed` | `unsupported`), `raw_payload` (jsonb), `warnings` (text[]), `created_at`, `updated_at`.
- [ ] `media_streams`: `id`, `node_id` (FK), `stream_index` (int), `stream_type` (enum: `video` | `audio` | `subtitle`), `codec` (text), `width` (int, nullable), `height` (int, nullable), `duration_ms` (bigint, nullable), `bitrate_bps` (bigint, nullable), `frame_rate` (text, nullable), `language` (text, nullable), `created_at`.
- [ ] Add normalized typed columns to `nodes` or a separate `node_metadata` table (as decided in Story 4.1): `detected_mime`, `media_kind`, `duration_ms`, `width`, `height`, `capture_time`, `page_count`, `orientation`, `has_gps`.
- [ ] Unique constraint on `(node_id, extractor_name)` — only one record per extractor per file.

---

### Story 4.5 — libmagic / MIME Detection Adapter

As a developer, I want a MIME detection module in `crates/media` so that every file gets an accurate content-based MIME type. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] `crates/media` exports a `detect_mime(path: &Path) -> Result<String>` function.
- [ ] Uses `libmagic` (via `tree_magic_mini`, `file` command, or an FFI binding) to detect MIME from the file's content bytes.
- [ ] Falls back to `application/octet-stream` if detection fails.
- [ ] Does **not** trust file extensions.
- [ ] Tests: verify correct MIME for JPEG, PNG, PDF, MP4, MP3, DOCX, and an extensionless file.

---

### Story 4.6 — ExifTool Adapter

As a developer, I want an ExifTool adapter in `crates/media` so that images and raw files get rich EXIF/IPTC/XMP metadata. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `crates/media` exports an `extract_exif(path: &Path) -> Result<ExifResult>` function.
- [ ] Invokes `exiftool -json -n <path>` as a child process.
- [ ] Enforces a timeout (configurable, default: 30s) and kills the process if exceeded.
- [ ] Enforces max output size (e.g., 5 MB) to prevent memory exhaustion.
- [ ] Parses the JSON output into a structured `ExifResult` with normalized fields: `width`, `height`, `orientation`, `capture_time`, `camera_make`, `camera_model`, `gps_latitude`, `gps_longitude`, `color_space`.
- [ ] Preserves the full raw JSON as `raw_payload` for storage in `metadata_records`.
- [ ] Records warnings for missing or suspicious fields.
- [ ] Tests with representative JPEG, PNG, and a raw camera file (e.g., CR2 or ARW).

---

### Story 4.7 — ffprobe Adapter

As a developer, I want an ffprobe adapter in `crates/media` so that video and audio files get codec, stream, and duration metadata. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `crates/media` exports `extract_ffprobe(path: &Path) -> Result<FfprobeResult>`.
- [ ] Invokes `ffprobe -v quiet -print_format json -show_format -show_streams <path>`.
- [ ] Timeout: 60s (configurable). Max output: 5 MB.
- [ ] Parses into `FfprobeResult` with: `container_format`, `duration_ms`, `total_bitrate`, and a `Vec<StreamInfo>` with per-stream `codec`, `type`, `width`, `height`, `frame_rate`, `bitrate`, `language`.
- [ ] Populates `media_streams` table rows.
- [ ] Tests with representative MP4 (H.264 + AAC), MKV, MP3, and M4A files.

---

### Story 4.8 — Apache Tika Adapter

As a developer, I want a Tika adapter in `crates/media` so that PDFs and office documents get title, author, page count, and other properties. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `crates/media` exports `extract_tika(path: &Path, tika_url: &str) -> Result<TikaResult>`.
- [ ] Sends the file to Tika's `/meta` endpoint via HTTP PUT with `Accept: application/json`.
- [ ] Timeout: 60s. Max response: 5 MB.
- [ ] Parses into `TikaResult` with normalized fields: `title`, `author`, `creation_date`, `modification_date`, `page_count`, `word_count`.
- [ ] Preserves full Tika JSON as `raw_payload`.
- [ ] Tests with representative PDF and DOCX files.

---

### Story 4.9 — Metadata Extraction Job Handler

As a developer, I want the worker to handle `metadata_extraction` jobs so that metadata is extracted for every ingested file. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] When a `metadata_extraction` job is claimed, the handler: retrieves the file from storage to a temp path, runs MIME detection, selects the appropriate extractor(s) based on MIME (exiftool for images, ffprobe for video/audio, tika for documents), runs the extractor(s), inserts `metadata_records` and `media_streams` rows, updates normalized columns on the node, and marks the job completed.
- [ ] If the MIME type doesn't match any specialized extractor, a generic `metadata_records` row is created with `status = unsupported` containing only MIME, size, and checksum.
- [ ] If an extractor fails, the job is marked failed with the error; the file remains accessible with whatever metadata was extracted.
- [ ] Extractor concurrency is bounded (max 1 ExifTool + 1 ffprobe + 1 Tika at a time, configurable) to respect 4 GB RAM.
- [ ] Tests: enqueue a job for a JPEG, process it, verify `metadata_records` and normalized fields.

---

### Story 4.10 — Gradual Reprocessing

As a developer, I want a mechanism to re-extract metadata when an extractor version changes so that old files benefit from improved extractors. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] A function `enqueue_reprocessing(extractor_name, old_version)` creates low-priority `metadata_extraction` jobs for all files whose `metadata_records` for that extractor have a version less than the current.
- [ ] Reprocessing jobs have lower priority than new-file metadata jobs.
- [ ] Reprocessing runs gradually (e.g., max 10 jobs enqueued at a time) to avoid flooding the queue.
- [ ] The reprocessing is idempotent: running it twice doesn't create duplicate jobs.
- [ ] Can be triggered via an internal API or admin endpoint: `POST /api/admin/reprocess?extractor=exiftool`.

---

### Story 4.11 — Metadata & Details API

As an API client, I want endpoints to retrieve file metadata and processing status so that the UI can display detailed file information. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `GET /api/files/:node_id` returns the node with all normalized metadata fields plus `processing_status` (derived from job state: `processing`, `ready`, `partially_processed`, `failed`).
- [ ] `GET /api/files/:node_id/metadata` returns all `metadata_records` for the file (raw payloads included or excluded via a `?raw=true` query param).
- [ ] `GET /api/files/:node_id/streams` returns `media_streams` for video/audio files.
- [ ] GPS coordinates are included when available; no location reverse-geocoding in v1.

---

### Story 4.12 — File Details Panel UI

As a user, I want a details side panel showing file metadata so that I can inspect file properties without opening a separate page. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Selecting a single file and clicking "Details" (or pressing a shortcut) opens a right-side panel.
- [ ] The panel displays: file name, type icon, size, MIME type, created/modified dates, checksum (truncated with copy button).
- [ ] For images: dimensions, orientation, camera make/model, capture time, GPS coordinates (if present).
- [ ] For video/audio: duration, codec(s), resolution, bitrate, stream list.
- [ ] For documents: title, author, page count, creation/modification dates.
- [ ] Processing status is shown with an appropriate indicator (spinner for `processing`, checkmark for `ready`, warning for `failed`).
- [ ] The panel works in both themes.
- [ ] Closing the panel or selecting a different file updates the content.

---

## Epic 5 — On-Demand Previews

**Goal:** Supported file types can be previewed in the browser. Previews are generated on first request, cached, and served efficiently.

**Sprint Capacity Estimate:** 2–3 sprints

---

### Story 5.1 — Resolve Preview Questions

As a developer, I want M5 questions decided (DOCX renderer, RAW decoder) so that preview implementation tools are chosen. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] Decision recorded for DOCX/office preview tool (headless LibreOffice vs dedicated converter) with ARM64 benchmarks.
- [ ] Decision recorded for raw camera image decoder with representative test files.
- [ ] `questions.md` M5 section cleared.

---

### Story 5.2 — Derived Artifacts Schema

As a developer, I want a `derived_artifacts` table for cached previews and thumbnails so that generated previews are tracked and reusable. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] `derived_artifacts`: `id`, `node_id` (FK), `artifact_type` (enum: `thumbnail` | `preview`), `format` (text, e.g., `image/webp`, `image/jpeg`, `application/pdf`), `width` (int, nullable), `height` (int, nullable), `storage_key` (text), `byte_size` (bigint), `generator_version` (text), `state` (enum: `generating` | `ready` | `failed`), `created_at`.
- [ ] Unique constraint on `(node_id, artifact_type)`.
- [ ] DB queries: `get_artifact`, `create_or_update_artifact`.

---

### Story 5.3 — Thumbnail Generator

As a developer, I want a thumbnail generator producing ~256×256 images so that the file table can show visual thumbnails for images and videos. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `crates/media` exports `generate_thumbnail(source: &Path, dest: &Path, max_size: u32) -> Result<ThumbnailResult>`.
- [ ] For JPEG/PNG/WebP/GIF: resize to fit within `max_size × max_size` preserving aspect ratio. Output as WebP.
- [ ] For video: extract a frame at ~10% of duration using `ffmpeg -ss <time> -i <input> -frames:v 1 -vf scale=... <output>`. Output as WebP.
- [ ] For raw camera images: use the decided decoder (libraw-based) to extract an embedded preview or decode at reduced resolution.
- [ ] Timeout: 30s per file. Memory limit awareness for 4 GB host.
- [ ] Returns `ThumbnailResult { width, height, format, byte_size }`.
- [ ] Tests with JPEG, PNG, GIF, MP4, and one raw file.

---

### Story 5.4 — Image & Animated GIF Preview

As a developer, I want image preview generation and serving so that users can view images without downloading the original. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] For JPEG/PNG/WebP: serve the original directly (it is browser-native).
- [ ] For animated GIF: serve the original with animation intact.
- [ ] For raw camera images: generate a full-resolution JPEG/WebP preview using the RAW decoder, cache as a derived artifact, and serve it.
- [ ] Large originals (e.g., > 20 MP) get a resized preview (max 2048px on the longest side) to save bandwidth.
- [ ] Correct `Content-Type` and `Cache-Control` headers on preview responses.

---

### Story 5.5 — Native Video & Audio Preview

As a developer, I want video and audio playback using browser-native codecs so that users can play media without transcoding. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] The preview endpoint for video/audio serves the original file with HTTP range support (reuses Story 2.9's download logic with `Content-Disposition: inline`).
- [ ] The UI renders a `<video>` or `<audio>` element pointing to the preview URL.
- [ ] If the browser can't play the codec, the UI shows "Preview not available — download instead" with a download button.
- [ ] No transcoding occurs in v1 — this is a hard constraint.
- [ ] Correct MIME types are set (`video/mp4`, `audio/mpeg`, etc.).

---

### Story 5.6 — PDF Preview

As a developer, I want PDF files to render in the browser so that users can view documents without downloading. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] The preview endpoint serves the original PDF with `Content-Type: application/pdf` and `Content-Disposition: inline`.
- [ ] The UI embeds it using `<iframe>` or `<embed>` (relying on the browser's built-in PDF renderer).
- [ ] If the PDF fails to load, fallback to a download button.
- [ ] Response headers include `X-Content-Type-Options: nosniff`.

---

### Story 5.7 — DOCX Preview

As a developer, I want DOCX files converted to a browser-viewable format so that users can preview office documents. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Using the decided tool (e.g., headless LibreOffice `soffice --convert-to pdf`), convert DOCX to PDF.
- [ ] Cache the converted PDF as a derived artifact.
- [ ] Serve the cached PDF via the preview endpoint.
- [ ] Timeout: 120s for conversion. Max concurrent conversions: 1 (to protect 4 GB RAM).
- [ ] If conversion fails, mark the artifact as `failed` and show "Preview not available" in the UI.
- [ ] Tests with a representative DOCX file.

---

### Story 5.8 — Preview Request & Status API

As an API client, I want endpoints to request, check, and retrieve previews so that previews are generated on demand and their status is queryable. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `GET /api/files/:node_id/preview` — if a cached preview exists, return it (with caching headers). If not, return `202 Accepted` with `{ "status": "generating", "job_id": UUID }` and enqueue a `preview_generation` job.
- [ ] `GET /api/files/:node_id/thumbnail` — same pattern for thumbnails.
- [ ] `GET /api/jobs/:job_id` — returns job status (for polling).
- [ ] When the preview is ready, subsequent requests to `/preview` return the cached artifact directly.
- [ ] If the file type is unsupported for preview, return `404` with `{ "error": "preview_not_supported" }`.

---

### Story 5.9 — Preview Generation Job Handler

As a developer, I want the worker to handle `preview_generation` jobs so that previews are generated in the background. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] When a `preview_generation` job is claimed: retrieve the file, determine preview strategy from MIME, generate the preview (thumbnail or full preview), store it via `StorageBackend` in the `artifacts/` namespace, insert/update a `derived_artifacts` row with `state = ready`, and mark the job completed.
- [ ] If generation fails, mark `derived_artifacts` as `failed` and the job as `failed`.
- [ ] Respect concurrency limits: max 2 concurrent preview generations (configurable).
- [ ] Clean up temp files after generation.
- [ ] Tests: enqueue preview for a JPEG, process, verify the artifact exists and is retrievable.

---

### Story 5.10 — Preview UI (Modal/Lightbox)

As a user, I want to preview a file by double-clicking it or pressing a "Preview" button so that I can view files quickly without downloading. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Double-clicking a file row opens a preview modal/lightbox.
- [ ] The modal shows a loading spinner while the preview is being generated.
- [ ] For images: displays the preview image (zoomable).
- [ ] For video: shows a `<video>` player with controls.
- [ ] For audio: shows an `<audio>` player with controls.
- [ ] For PDF: embeds the PDF viewer.
- [ ] For DOCX: shows the converted PDF.
- [ ] For unsupported types: shows file info and a "Download" button.
- [ ] Pressing Escape or clicking outside closes the modal.
- [ ] Arrow keys navigate to the next/previous file in the table.
- [ ] Works in both themes.

---

## Epic 6 — Complete v1 File Management & UI

**Goal:** Trash, restore, permanent deletion, favorites, sorting, filters, command bar, and all remaining UI features are implemented.

**Sprint Capacity Estimate:** 2–3 sprints

---

### Story 6.1 — Resolve Command Bar Questions

As a developer, I want M6 questions decided (command list, parsing rules) so that command bar implementation is clear. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] Decision recorded for which commands are in v1 (`pwd`, `ls`, `cd`, `mkdir`, `mv`, `rm`, `restore`, `open` — or a subset).
- [ ] Decision recorded for parsing: quoting, escaping, relative/absolute paths, autocomplete, history, confirmation for destructive commands.
- [ ] `questions.md` M6 section cleared.

---

### Story 6.2 — Trash & Restore

As a user, I want to move items to trash and restore them so that deletion is reversible. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Migration creates `trash_entries`: `id`, `node_id` (FK, unique), `original_parent_id` (FK), `trashed_at` (timestamptz), `scheduled_purge_at` (timestamptz, default: `trashed_at + 30 days`).
- [ ] `DELETE /api/nodes/:id` (or `POST /api/nodes/:id/trash`) moves a node to trash: sets `lifecycle_state = trashed`, creates a `trash_entries` row.
- [ ] Trashing a folder trashes all its descendants recursively in one transaction.
- [ ] `POST /api/nodes/:id/restore` restores a node: sets `lifecycle_state = active`, deletes the `trash_entries` row. If the original parent no longer exists (was permanently deleted), restore to root.
- [ ] `GET /api/trash` lists all trashed items with `trashed_at` and `scheduled_purge_at`.
- [ ] Trashed items are excluded from normal folder listings.
- [ ] Trashed items still count toward disk usage.
- [ ] Tests: trash, verify exclusion from listing, restore, verify re-inclusion.

---

### Story 6.3 — Permanent Deletion

As a user, I want to permanently delete trashed items and have their bytes freed immediately so that I can reclaim disk space. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `DELETE /api/nodes/:id/permanent` (or `POST /api/nodes/:id/purge`) — only valid for trashed items.
- [ ] Enqueues a `permanent_deletion` job (handled by the worker).
- [ ] The deletion job: deletes the original from storage, deletes all derived artifacts from storage, deletes `metadata_records`, `media_streams`, `derived_artifacts`, `file_objects`, `trash_entries`, and finally the `nodes` row. All in a transaction where possible.
- [ ] For folders: recursively deletes all descendants.
- [ ] The operation is idempotent: deleting an already-deleted node returns `200` or `204`.
- [ ] Tests: permanently delete a file, verify storage files are gone, verify DB rows are gone.

---

### Story 6.4 — Automatic 30-Day Trash Cleanup

As a system, I want trashed items auto-purged after 30 days so that disk space is reclaimed without manual intervention. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] A periodic task (in the worker, every hour) queries `trash_entries WHERE scheduled_purge_at <= now()`.
- [ ] For each expired entry, enqueues a `permanent_deletion` job.
- [ ] Batch size is limited (e.g., 50 per run) to avoid overwhelming the worker.
- [ ] The cleanup is idempotent.
- [ ] Tests: create a trashed item with `scheduled_purge_at` in the past, run cleanup, verify it's permanently deleted.

---

### Story 6.5 — Favorites

As a user, I want to favorite files and folders and see them in a favorites view so that I can quickly access important items. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] Migration creates `favorites`: `node_id` (FK, PK), `created_at`.
- [ ] `POST /api/nodes/:id/favorite` adds a favorite (idempotent).
- [ ] `DELETE /api/nodes/:id/favorite` removes a favorite.
- [ ] `GET /api/favorites` lists all favorited nodes with their details (sorted by favorited time).
- [ ] The file table shows a star icon on favorited items.
- [ ] Clicking the star toggles the favorite status.
- [ ] The "Favorites" sidebar link navigates to the favorites listing.
- [ ] Trashing a favorited item removes the favorite.

---

### Story 6.6 — Column Sorting

As a user, I want to sort the file table by clicking column headers so that I can find files by name, date, size, or type. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] Clicking a column header sorts by that column (ascending). Clicking again reverses to descending. A third click returns to default.
- [ ] Sortable columns: Name, Kind/Type, Size, Date Modified, Date Created.
- [ ] Sort direction is indicated by an arrow icon in the column header.
- [ ] Sorting is done server-side: `GET /api/folders/:id/children?sort=name&order=asc`.
- [ ] Folders always sort before files (within the chosen sort column).
- [ ] Sort preferences persist in the URL query string (so they survive navigation).

---

### Story 6.7 — Kind Filters

As a user, I want to filter the file table by file kind (folders, images, documents, video, audio) so that I can narrow down what I'm looking at. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] A filter bar (above the table or in the toolbar) has toggle buttons for: All, Folders, Images, Documents, Video, Audio.
- [ ] Filters map to MIME prefixes or a `media_kind` enum on the server.
- [ ] `GET /api/folders/:id/children?kind=image` returns only matching items.
- [ ] Multiple filters can be combined (e.g., images + video).
- [ ] Active filters are visually highlighted.
- [ ] Filter state persists in the URL query string.

---

### Story 6.8 — Multi-Item Actions Toolbar

As a user, I want a toolbar that appears when items are selected with batch actions so that I can operate on multiple files at once. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] When ≥ 1 item is selected, a contextual toolbar appears (replacing or overlaying the default toolbar).
- [ ] Actions: "Move to…", "Move to Trash", "Favorite" / "Unfavorite", "Download" (single file only, or zipped for multiple — zip is deferred, so single-file download for now).
- [ ] Each action applies to all selected items.
- [ ] "Move to Trash" with multiple items trashes all in one transaction.
- [ ] After a batch action, the selection is cleared and the table refreshes.
- [ ] Keyboard shortcut: Delete/Backspace moves selected items to trash (with confirmation).

---

### Story 6.9 — Trash View UI

As a user, I want a dedicated trash view showing trashed items so that I can review, restore, or permanently delete items. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] The "Trash" sidebar link navigates to `/trash`.
- [ ] The trash view shows the same table layout but with columns: Name, Original Location, Trashed Date, Days Until Deletion.
- [ ] Context menu actions: "Restore", "Delete Permanently".
- [ ] A "Empty Trash" button permanently deletes all trashed items (with a confirmation dialog: "This will permanently delete X items. This action cannot be undone.").
- [ ] No "Create Folder" or "Upload" actions in the trash view.

---

### Story 6.10 — Command Bar

As a user, I want a command bar where I can type filesystem-like commands so that I can perform actions quickly via keyboard. **Estimated: 8 points.**

**Acceptance Criteria:**

- [ ] A command bar input is visible at the top or bottom of the content area (per design).
- [ ] Typing a command and pressing Enter executes it.
- [ ] Supported commands (as decided in Story 6.1, e.g.): `pwd` (print current path), `ls [path]` (list contents), `cd <path>` (navigate), `mkdir <name>` (create folder), `mv <source> <dest>` (move/rename), `rm <target>` (trash), `restore <target>` (restore from trash), `open <target>` (preview/open).
- [ ] Paths support relative (`..`, `./`) and absolute (`/`) notation within the virtual hierarchy.
- [ ] Basic autocomplete for paths: pressing Tab completes the current path segment by querying `GET /api/folders/:id/children`.
- [ ] Command history: Up/Down arrows cycle through recent commands (stored in `localStorage`, last 50).
- [ ] Destructive commands (`rm`) require confirmation or an `--force` flag.
- [ ] Errors are displayed inline below the command bar (e.g., "No such folder: /photos/2025").
- [ ] Quoting supports spaces in names: `mkdir "My Photos"` or `mkdir My\ Photos`.

---

### Story 6.11 — Storage Usage Display

As a user, I want to see how much storage is used broken down by category so that I know what's consuming space. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `GET /api/storage/usage` returns `{ "total_bytes", "used_bytes", "available_bytes", "originals_bytes", "artifacts_bytes", "trash_bytes", "usage_percent" }`.
- [ ] The sidebar shows a storage meter (progress bar) with used/total (e.g., "1.2 TB / 5 TB used").
- [ ] Clicking the meter (or a "Details" link) shows a breakdown: Originals, Previews/Thumbnails, Trash.
- [ ] The meter color changes at 80% (warning) and 90% (critical).

---

### Story 6.12 — Status Footer

As a user, I want a footer bar showing item count, selection info, and processing status so that I always know the state of the current view. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] A footer at the bottom of the content area shows: `"X items"` (total in current view), `"Y selected"` (when items are selected), and processing indicator (e.g., `"3 files processing"` if any jobs are active).
- [ ] Processing count is derived from `GET /api/jobs?state=pending,leased&count=true` or embedded in folder listing response.
- [ ] Footer renders in both themes.

---

### Story 6.13 — Transient Toast Notifications

As a user, I want brief toast messages for completed actions and recoverable errors so that I get feedback without blocking my workflow. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] A toast notification system renders messages at the bottom-right of the viewport.
- [ ] Toasts auto-dismiss after 5 seconds or can be manually dismissed.
- [ ] Types: `success` (green accent), `error` (red accent), `info` (neutral).
- [ ] Used for: "Folder created", "3 items moved to trash", "Upload failed: name conflict", etc.
- [ ] Max 3 toasts visible at once; older ones are pushed out.
- [ ] Works in both themes.

---

## Epic 7 — v1 Stabilization

**Goal:** The application is reliable, documented, and tested on the target hardware. All v1 behaviors are verified.

**Sprint Capacity Estimate:** 2 sprints

---

### Story 7.1 — End-to-End Test Suite

As a developer, I want an E2E test covering the full lifecycle (folder → upload → metadata → preview → trash → restore → delete) so that the core workflow is verified as a whole. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] An automated test (using a test framework like Playwright or a Rust integration test against the real API) performs: create a folder, resumable-upload a file, verify metadata extraction completes, request and verify preview generation, download and verify byte-for-byte integrity, trash the file, list trash to confirm presence, restore the file, permanently delete the file, verify storage is freed.
- [ ] The test runs against a real PostgreSQL and storage backend (docker compose test environment).
- [ ] The test passes on x86-64 CI and on ARM64 (Raspberry Pi or emulated).

---

### Story 7.2 — Import End-to-End Test

As a developer, I want an E2E test for the watched-folder import pipeline so that import reliability is verified. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] Test: configure an import source, place a file in the watch directory, wait for stable detection and import, verify the node exists with correct metadata, restart the worker, verify no duplicate node is created, place a second file with the same name as an existing file, verify the conflict is recorded as an error.
- [ ] Test runs against real services.

---

### Story 7.3 — Edge Case & Failure Mode Tests

As a developer, I want tests for low disk, missing storage, interrupted uploads, worker crashes, and malformed files so that error handling is verified. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Test: mock disk at 91% → upload initiation returns `507`.
- [ ] Test: start API with unreachable PostgreSQL → exits with non-zero code.
- [ ] Test: start API with missing `STORAGE_ROOT` → exits with non-zero code.
- [ ] Test: upload 3 chunks, kill the API, restart, resume from chunk 4 → succeeds.
- [ ] Test: submit a malformed/corrupt file to metadata extraction → job fails, file remains accessible with generic metadata.
- [ ] Test: submit a file that causes ExifTool to hang → killed after timeout, job fails gracefully.
- [ ] Test: permanently delete a file whose storage key is already missing → job completes (idempotent).
- [ ] Test: trash cleanup with 100 expired items → all purged without errors.

---

### Story 7.4 — ARM64 Raspberry Pi Validation

As a developer, I want all tests passing on the Raspberry Pi 5 target so that the application works on the intended hardware. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] Full test suite runs on a Raspberry Pi 5 with 4 GB RAM.
- [ ] No OOM kills during tests (monitored via `dmesg` or `journalctl`).
- [ ] Worker concurrency tuned: document recommended `WORKER_CONCURRENCY` for 4 GB (likely 1–2).
- [ ] All container images build for `linux/arm64`.
- [ ] ExifTool, ffprobe, and Tika are confirmed available and functional on ARM64.

---

### Story 7.5 — x86-64 Build & Compatibility

As a developer, I want the full stack to build and pass tests on x86-64 so that development isn't locked to ARM. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] `cargo build --release --target x86_64-unknown-linux-gnu` succeeds.
- [ ] All Rust unit and integration tests pass on x86-64.
- [ ] Docker Compose dev setup works on an x86-64 machine (macOS via Docker Desktop or native Linux).
- [ ] Frontend build and tests pass on x86-64.

---

### Story 7.6 — Performance Tuning Documentation

As a developer or self-hoster, I want documented configuration recommendations for the 4 GB Raspberry Pi so that performance is acceptable on the target hardware. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] A `docs/performance.md` documents: recommended `WORKER_CONCURRENCY` (e.g., 2), PostgreSQL shared_buffers / work_mem settings for 4 GB host, max concurrent ExifTool / ffprobe / Tika processes, expected metadata extraction time per file type (measured), expected thumbnail generation time (measured).
- [ ] Memory usage of the API + worker + PostgreSQL + Tika under load is documented (measured with `htop` / `free` on the Pi).

---

### Story 7.7 — Developer Documentation

As a new developer, I want a comprehensive README and contributing guide so that I can set up and start developing quickly. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `README.md` covers: project overview, architecture diagram, prerequisites (Rust, Node, Docker), setup instructions (clone, `docker compose up`, `cargo run`, `npm run dev`), running tests, environment variables reference, and project structure.
- [ ] `docs/setup.md` has detailed step-by-step setup for macOS, Linux (x86-64), and Raspberry Pi (ARM64).
- [ ] `docs/architecture.md` documents the crate structure, data flow, and key design decisions.
- [ ] `docs/supported-formats.md` lists all supported file types with their metadata extractors and preview capabilities.
- [ ] `docs/known-limitations.md` lists all v1 exclusions (from README.md § 3) in user-facing language.

---

### Story 7.8 — Plan Reconciliation

As a project owner, I want `README.md`, `questions.md`, and `deferred.md` reconciled with shipped behavior so that documentation is accurate and up to date. **Estimated: 1 point.**

**Acceptance Criteria:**

- [ ] All milestones in `README.md` are marked complete with links to relevant code/decisions.
- [ ] All questions in `questions.md` are resolved (file should be empty or contain only v2 items).
- [ ] `deferred.md` is reviewed and still accurate.
- [ ] Any v1 behavior that deviated from `README.md` is documented with rationale.

---

## Summary

| Epic | Milestone | Stories | Estimated Points |
|---|---|---|---|
| 0 — Foundations & Scaffold | M0 | 10 | 25 |
| 1 — Persist & Browse Folders | M1 | 12 | 37 |
| 2 — Resumable Upload & Download | M2 | 14 | 50 |
| 3 — Watched-Folder Import | M3 | 8 | 27 |
| 4 — Metadata Extraction | M4 | 12 | 44 |
| 5 — On-Demand Previews | M5 | 10 | 35 |
| 6 — Complete v1 File Management & UI | M6 | 13 | 42 |
| 7 — v1 Stabilization | M7 | 8 | 24 |
| **Total** | | **87** | **284** |

> [!TIP]
> At a velocity of ~30 points/sprint with 2-week sprints, this is approximately **10 sprints (~20 weeks)** of work. Adjust based on measured velocity after the first 2 sprints.
