# Epic 1 — Persist & Browse Folders


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
