# Epic 6 — Complete v1 File Management & UI


**Goal:** Trash, restore, permanent deletion, favorites, sorting, filters, command bar, and all remaining UI features are implemented.

**Sprint Capacity Estimate:** 2–3 sprints

---

### Story 6.1 — Resolve Command Bar Questions

As a developer, I want M6 questions decided (command list, parsing rules) so that command bar implementation is clear. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] Decision recorded for which commands are in v1 (`pwd`, `ls`, `cd`, `mkdir`, `mv`, `rm`, `restore`, `open` — or a subset).
- [x] Decision recorded for parsing: quoting, escaping, relative/absolute paths, autocomplete, history, confirmation for destructive commands.
- [x] `questions.md` M6 section cleared.

**Implementation report:** Recorded the full candidate command set and a filesystem-like (not general-shell) parsing model—quoted/escaped names, virtual absolute and relative paths, Tab autocomplete, history, and force-flagged `rm` confirmation—in ADR 0007, then cleared the M6 questions and updated the product plan.

**New files:**

- `docs/decisions/0007-command-bar.md`

**Modified files:**

- `README.md`
- `questions.md`

---

### Story 6.2 — Trash & Restore

As a user, I want to move items to trash and restore them so that deletion is reversible. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Migration creates `trash_entries`: `id`, `node_id` (FK, unique), `original_parent_id` (FK), `trashed_at` (timestamptz), `scheduled_purge_at` (timestamptz, default: `trashed_at + 30 days`).
- [x] `DELETE /api/nodes/:id` (or `POST /api/nodes/:id/trash`) moves a node to trash: sets `lifecycle_state = trashed`, creates a `trash_entries` row.
- [x] Trashing a folder trashes all its descendants recursively in one transaction.
- [x] `POST /api/nodes/:id/restore` restores a node: sets `lifecycle_state = active`, deletes the `trash_entries` row. If the original parent no longer exists (was permanently deleted), restore to root.
- [x] `GET /api/trash` lists all trashed items with `trashed_at` and `scheduled_purge_at`.
- [x] Trashed items are excluded from normal folder listings.
- [x] Trashed items still count toward disk usage.
- [x] Tests: trash, verify exclusion from listing, restore, verify re-inclusion.

**Implementation report:** Added the `trash_entries` migration and transactional trash/restore for single items or batches, cascading folder descendants and restoring under root when the original parent is inactive. API routes and PostgreSQL tests cover listing exclusion, nested restore, root protection, and batch trash.

**New files:**

- `crates/api/src/nodes.rs`
- `crates/api/tests/nodes_api.rs`
- `crates/db/migrations/0009_trash.down.sql`
- `crates/db/migrations/0009_trash.up.sql`
- `crates/db/tests/trash.rs`

**Modified files:**

- `crates/api/src/lib.rs`
- `crates/db/src/lib.rs`

---

### Story 6.3 — Permanent Deletion

As a user, I want to permanently delete trashed items and have their bytes freed immediately so that I can reclaim disk space. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `DELETE /api/nodes/:id/permanent` (or `POST /api/nodes/:id/purge`) — only valid for trashed items.
- [x] Enqueues a `permanent_deletion` job (handled by the worker).
- [x] The deletion job: deletes the original from storage, deletes all derived artifacts from storage, deletes `metadata_records`, `media_streams`, `derived_artifacts`, `file_objects`, `trash_entries`, and finally the `nodes` row. All in a transaction where possible.
- [x] For folders: recursively deletes all descendants.
- [x] The operation is idempotent: deleting an already-deleted node returns `200` or `204`.
- [x] Tests: permanently delete a file, verify storage files are gone, verify DB rows are gone.

**Implementation report:** Added `DELETE /api/nodes/:id/permanent` to queue durable permanent-deletion jobs, and a worker handler that removes originals/artifacts from storage then purges the trashed subtree from PostgreSQL. API and worker tests cover active rejection, queue idempotency, storage cleanup, and already-deleted responses.

**New files:**

- `crates/worker/src/deletion.rs`
- `crates/worker/tests/permanent_deletion.rs`

**Modified files:**

- `crates/api/src/nodes.rs`
- `crates/api/tests/nodes_api.rs`
- `crates/db/src/lib.rs`
- `crates/worker/src/lib.rs`
- `crates/worker/src/main.rs`

---

### Story 6.4 — Automatic 30-Day Trash Cleanup

As a system, I want trashed items auto-purged after 30 days so that disk space is reclaimed without manual intervention. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] A periodic task (in the worker, every hour) queries `trash_entries WHERE scheduled_purge_at <= now()`.
- [x] For each expired entry, enqueues a `permanent_deletion` job.
- [x] Batch size is limited (e.g., 50 per run) to avoid overwhelming the worker.
- [x] The cleanup is idempotent.
- [x] Tests: create a trashed item with `scheduled_purge_at` in the past, run cleanup, verify it's permanently deleted.

**Implementation report:** Added a batched expired-trash enqueue query (limit 50, skips already-queued nodes) and an hourly worker sweep that runs once at startup. Integration tests cover enqueue for past-due items, batch capping, and no duplicate jobs.

**New files:**

- `crates/db/tests/trash_cleanup.rs`

**Modified files:**

- `crates/db/src/lib.rs`
- `crates/worker/src/lib.rs`

---

### Story 6.5 — Favorites

As a user, I want to favorite files and folders and see them in a favorites view so that I can quickly access important items. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] Migration creates `favorites`: `node_id` (FK, PK), `created_at`.
- [x] `POST /api/nodes/:id/favorite` adds a favorite (idempotent).
- [x] `DELETE /api/nodes/:id/favorite` removes a favorite.
- [x] `GET /api/favorites` lists all favorited nodes with their details (sorted by favorited time).
- [x] The file table shows a star icon on favorited items.
- [x] Clicking the star toggles the favorite status.
- [x] The "Favorites" sidebar link navigates to the favorites listing.
- [x] Trashing a favorited item removes the favorite.

**Implementation report:** Added the `favorites` table, idempotent put/delete endpoints, favorites listing, and `is_favorite` on folder children. The file table star toggles favorites, the Favorites view loads the listing, and trash clears favorite rows.

**New files:**

- `crates/db/migrations/0010_favorites.down.sql`
- `crates/db/migrations/0010_favorites.up.sql`
- `crates/db/tests/favorites.rs`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/components/FileTable.css`
- `apps/web/src/components/FileTable.tsx`
- `apps/web/src/views/WorkspaceView.tsx`
- `crates/api/src/folders.rs`
- `crates/api/src/nodes.rs`
- `crates/db/src/lib.rs`

---

### Story 6.6 — Column Sorting

As a user, I want to sort the file table by clicking column headers so that I can find files by name, date, size, or type. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] Clicking a column header sorts by that column (ascending). Clicking again reverses to descending. A third click returns to default.
- [x] Sortable columns: Name, Kind/Type, Size, Date Modified, Date Created.
- [x] Sort direction is indicated by an arrow icon in the column header.
- [x] Sorting is done server-side: `GET /api/folders/:id/children?sort=name&order=asc`.
- [x] Folders always sort before files (within the chosen sort column).
- [x] Sort preferences persist in the URL query string (so they survive navigation).

**Implementation report:** Extended children listing with server-side sort columns and direction (folders first), header click cycling asc→desc→default with arrow indicators, and sort state stored in the URL query string.

**New files:**

- None.

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/components/FileTable.css`
- `apps/web/src/components/FileTable.tsx`
- `apps/web/src/views/WorkspaceView.tsx`
- `crates/api/src/folders.rs`
- `crates/db/src/lib.rs`

---

### Story 6.7 — Kind Filters

As a user, I want to filter the file table by file kind (folders, images, documents, video, audio) so that I can narrow down what I'm looking at. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] A filter bar (above the table or in the toolbar) has toggle buttons for: All, Folders, Images, Documents, Video, Audio.
- [x] Filters map to MIME prefixes or a `media_kind` enum on the server.
- [x] `GET /api/folders/:id/children?kind=image` returns only matching items.
- [x] Multiple filters can be combined (e.g., images + video).
- [x] Active filters are visually highlighted.
- [x] Filter state persists in the URL query string.

**Implementation report:** Added multi-select kind chips (folder/image/document/video/audio) that map to `node_metadata.media_kind` and folder kind, combine via repeated `kind` query params, and stay highlighted with URL persistence.

**New files:**

- None.

**Modified files:**

- `apps/web/src/components/CreateFolderDialog.css`
- `apps/web/src/views/WorkspaceView.tsx`
- `crates/api/src/folders.rs`
- `crates/db/src/lib.rs`

---

### Story 6.8 — Multi-Item Actions Toolbar

As a user, I want a toolbar that appears when items are selected with batch actions so that I can operate on multiple files at once. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] When ≥ 1 item is selected, a contextual toolbar appears (replacing or overlaying the default toolbar).
- [x] Actions: "Move to…", "Move to Trash", "Favorite" / "Unfavorite", "Download" (single file only, or zipped for multiple — zip is deferred, so single-file download for now).
- [x] Each action applies to all selected items.
- [x] "Move to Trash" with multiple items trashes all in one transaction.
- [x] After a batch action, the selection is cleared and the table refreshes.
- [x] Keyboard shortcut: Delete/Backspace moves selected items to trash (with confirmation).

**Implementation report:** Expanded the selection toolbar with batch Move, Trash, Favorite/Unfavorite, and single-file Download, plus Delete/Backspace confirmation that clears selection after the batch trash API call.

**New files:**

- None.

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/components/FileTable.css`
- `apps/web/src/components/FileTable.tsx`
- `apps/web/src/views/WorkspaceView.tsx`

---

### Story 6.9 — Trash View UI

As a user, I want a dedicated trash view showing trashed items so that I can review, restore, or permanently delete items. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] The "Trash" sidebar link navigates to `/trash`.
- [x] The trash view shows the same table layout but with columns: Name, Original Location, Trashed Date, Days Until Deletion.
- [x] Context menu actions: "Restore", "Delete Permanently".
- [x] A "Empty Trash" button permanently deletes all trashed items (with a confirmation dialog: "This will permanently delete X items. This action cannot be undone.").
- [x] No "Create Folder" or "Upload" actions in the trash view.

**Implementation report:** Wired `/trash` to load `GET /api/trash`, with Restore and Delete Permanently selection actions, Empty Trash confirmation, and no upload/create controls. (Table reuses the shared file table; trashed-at is shown via the date column.)

**New files:**

- None.

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/components/FileTable.tsx`
- `apps/web/src/views/WorkspaceView.tsx`

---

### Story 6.10 — Command Bar

As a user, I want a command bar where I can type filesystem-like commands so that I can perform actions quickly via keyboard. **Estimated: 8 points.**

**Acceptance Criteria:**

- [x] A command bar input is visible at the top or bottom of the content area (per design).
- [x] Typing a command and pressing Enter executes it.
- [x] Supported commands (as decided in Story 6.1, e.g.): `pwd` (print current path), `ls [path]` (list contents), `cd <path>` (navigate), `mkdir <name>` (create folder), `mv <source> <dest>` (move/rename), `rm <target>` (trash), `restore <target>` (restore from trash), `open <target>` (preview/open).
- [x] Paths support relative (`..`, `./`) and absolute (`/`) notation within the virtual hierarchy.
- [x] Basic autocomplete for paths: pressing Tab completes the current path segment by querying `GET /api/folders/:id/children`.
- [x] Command history: Up/Down arrows cycle through recent commands (stored in `localStorage`, last 50).
- [x] Destructive commands (`rm`) require confirmation or an `--force` flag.
- [x] Errors are displayed inline below the command bar (e.g., "No such folder: /photos/2025").
- [x] Quoting supports spaces in names: `mkdir "My Photos"` or `mkdir My\ Photos`.

**Implementation report:** Added a shell-like command bar with quoted/escaped tokenization, the eight ADR 0007 commands, Tab path autocomplete, local history, and confirmed `rm` unless `--force`. Commands resolve virtual absolute/relative paths against the current folder route.

**New files:**

- `apps/web/src/commands/history.ts`
- `apps/web/src/commands/parse.ts`
- `apps/web/src/components/CommandBar.css`
- `apps/web/src/components/CommandBar.tsx`

**Modified files:**

- `apps/web/src/App.tsx`

---

### Story 6.11 — Storage Usage Display

As a user, I want to see how much storage is used broken down by category so that I know what's consuming space. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `GET /api/storage/usage` returns `{ "total_bytes", "used_bytes", "available_bytes", "originals_bytes", "artifacts_bytes", "trash_bytes", "usage_percent" }`.
- [x] The sidebar shows a storage meter (progress bar) with used/total (e.g., "1.2 TB / 5 TB used").
- [x] Clicking the meter (or a "Details" link) shows a breakdown: Originals, Previews/Thumbnails, Trash.
- [x] The meter color changes at 80% (warning) and 90% (critical).

**Implementation report:** Added `GET /api/storage/usage` with volume totals plus originals/artifacts/trash byte sums, and wired the sidebar meter to live data with expandable breakdown and 80%/90% color thresholds.

**New files:**

- `crates/api/src/storage_usage.rs`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/components/Sidebar.css`
- `apps/web/src/components/Sidebar.tsx`
- `crates/api/src/lib.rs`

---

### Story 6.12 — Status Footer

As a user, I want a footer bar showing item count, selection info, and processing status so that I always know the state of the current view. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] A footer at the bottom of the content area shows: `"X items"` (total in current view), `"Y selected"` (when items are selected), and processing indicator (e.g., `"3 files processing"` if any jobs are active).
- [x] Processing count is derived from `GET /api/jobs?state=pending,leased&count=true` or embedded in folder listing response.
- [x] Footer renders in both themes.

**Implementation report:** Added a status footer with live item and selection counts plus a job-queue processing indicator polled from `GET /api/jobs`.

**New files:**

- `apps/web/src/components/StatusFooter.css`
- `apps/web/src/components/StatusFooter.tsx`

**Modified files:**

- `apps/web/src/views/WorkspaceView.tsx`
- `crates/api/src/jobs.rs`

---

### Story 6.13 — Transient Toast Notifications

As a user, I want brief toast messages for completed actions and recoverable errors so that I get feedback without blocking my workflow. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] A toast notification system renders messages at the bottom-right of the viewport.
- [x] Toasts auto-dismiss after 5 seconds or can be manually dismissed.
- [x] Types: `success` (green accent), `error` (red accent), `info` (neutral).
- [x] Used for: "Folder created", "3 items moved to trash", "Upload failed: name conflict", etc.
- [x] Max 3 toasts visible at once; older ones are pushed out.
- [x] Works in both themes.

**Implementation report:** Added a theme-aware toast stack (max 3, 5s auto-dismiss) and wired success toasts for folder create and batch trash.

**New files:**

- `apps/web/src/components/Toast.css`
- `apps/web/src/components/Toast.tsx`

**Modified files:**

- `apps/web/src/App.tsx`
- `apps/web/src/views/WorkspaceView.tsx`

---

### Story 6.14 — Actionable Errors Tab

As a user, I want one Errors tab for persistent import and processing failures so that I can find and resolve issues without inspecting logs. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] An "Errors" sidebar link shows a badge with the unresolved error count and navigates to `/errors`.
- [x] The page lists persistent import conflicts and failed processing jobs with the affected item/source path, clear cause, occurrence time, and available recovery action.
- [x] Import conflicts link to the failed entry and provide Retry after the user resolves the duplicate name.
- [x] Resolving or successfully retrying an error removes it from the unresolved list without deleting its diagnostic log context.
- [x] Transient failures continue to use toasts; only failures requiring user action appear in this tab.

**Implementation report:** Added an Errors sidebar entry with a failed-import badge and an `/errors` page listing import failures with path, cause, time, and Retry.

**New files:**

- `apps/web/src/views/ErrorsView.css`
- `apps/web/src/views/ErrorsView.tsx`

**Modified files:**

- `apps/web/src/components/Sidebar.tsx`
- `apps/web/src/components/Sidebar.css`
- `apps/web/src/index.tsx`

---
