# Strife — Photos & Albums Scrum Epics and Stories

> Derived from the requirement to add a dedicated Photos tab to Strife using the album/library experience in the local `~/projects/fstop` repository as the interaction reference while keeping Strife as the system of record. Epic numbering continues from [`email.md`](email.md), which ends at Epic 22. Stories are ordered by dependency within each epic. Point estimates use a Fibonacci scale (1, 2, 3, 5, 8, 13). Acceptance criteria are written so a mid-level developer can implement and self-verify without ambiguity.

**Planning decisions:** Strife nodes and file objects remain the canonical originals · the Photos tab is a view over active Strife files whose normalized `media_kind` is `image` · no fstop database, ingestion pipeline, storage keys, authentication model, or photo copies are introduced · fstop's three-pane library, virtualized square grid, month navigation, keyboard cursor, EXIF inspector, and lightbox are adapted to Strife's shell and design tokens · thumbnails and previews reuse Strife's existing derived-artifact pipeline · photo mutations reuse Strife's favorite, move, trash, restore, download, and preview behavior · albums are virtual collections whose membership never moves or duplicates a file · capture time sorts ahead of file creation time, with deterministic fallbacks for missing EXIF · active files are included only after MIME/metadata classification identifies them as images · videos, face recognition, perceptual duplicate detection, editing, maps, sharing, tags, and smart albums are deferred from the initial Photos tab.

## fstop Adaptation Boundary

| fstop behavior                                                            | Strife treatment                                                                                                       |
| ------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| Three-pane library with source tree, photo grid, and inspector            | Adapt inside Strife's existing application shell; the global Strife sidebar remains visible.                           |
| Virtualized square thumbnail grid and month jump list                     | Port the interaction model to SolidJS using UUID-backed Strife photo records and keyset pagination.                    |
| Full EXIF data included with each photo page                              | Add a bounded Strife photo-list projection so selection never causes one metadata request per cell.                    |
| Thumbnail, preview, and original media routes                             | Reuse `/api/files/{id}/thumbnail`, `/preview`, `/preview-native`, and `/download`; do not add a second blob namespace. |
| Directory and album navigation                                            | Derive directories from the Strife node hierarchy and add virtual album membership linked to node IDs.                 |
| Upload, import, storage quota, authentication, trash, and download        | Reuse Strife's existing implementations; none are ported from fstop.                                                   |
| fstop tags, duplicates, pHash, command mode, and photo-specific ingestion | Deferred unless a later Strife plan explicitly adopts them.                                                            |

---

## Epic 23 — Photo Projection and Browse APIs

**Goal:** Strife exposes one stable, paginated photo-library contract backed by existing nodes, metadata, and artifacts before the new UI depends on it.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 23.1 — Record the Photos Architecture Decision

As a developer, I want the Photos boundary recorded so that adapting fstop does not accidentally create a second file-management system inside Strife. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] A new ADR follows the context, decision, alternatives, consequences, and date shape used by the existing Strife ADRs.
- [ ] The ADR states that nodes, file objects, node metadata, and derived artifacts remain canonical and that photo records are read projections rather than copied originals.
- [ ] The ADR records fstop as an interaction and implementation reference, identifies the components being adapted, and rejects importing its Go API, PostgreSQL schema, storage keys, authentication, ingest queue, and quota model.
- [ ] The ADR defines a photo as an active finalized file with normalized `media_kind = 'image'`; filename extensions alone never qualify a file.
- [ ] The ADR records capture-time ordering with deterministic fallbacks, keyset pagination, album membership semantics, and deletion behavior.
- [ ] The ADR states that albums are virtual collections: adding or removing a photo changes membership only, while move, rename, trash, restore, and permanent deletion continue through Strife's node APIs.
- [ ] The ADR records the initial exclusions listed in this plan, especially videos, editing, faces, pHash duplicates, maps, sharing, tags, and smart albums.
- [ ] `README.md` links the ADR and this implementation plan from the feature or processing documentation.

---

### Story 23.2 — Normalize Photo Inspector Metadata

As a user, I want useful camera and exposure data in the photo inspector so that the adapted fstop view is backed by queryable Strife metadata rather than raw JSON lookups. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] A reversible migration extends `node_metadata` with nullable `lens_model`, `f_number`, `exposure_time`, `iso`, and `focal_length_mm` fields using types and constraints that preserve valid EXIF values.
- [ ] A bounded 24-bin luminance histogram is either stored as a constrained normalized field or explicitly deferred in the ADR with a stable empty-state design; the UI never parses `raw_payload` directly.
- [ ] The ExifTool adapter normalizes the new fields while continuing to retain the complete raw payload and warnings.
- [ ] Metadata replacement remains idempotent and clears stale normalized values when a later successful extraction no longer reports them.
- [ ] Existing rows remain valid without a table rewrite, and files missing one or every new field remain browseable.
- [ ] `GET /api/files/{id}` returns the normalized fields so the existing details panel and the Photos inspector share one meaning.
- [ ] Adapter tests cover a representative JPEG and RAW payload, fractional exposure, missing values, invalid numeric values, and preservation of the raw payload.
- [ ] PostgreSQL integration coverage verifies constraints and replacement behavior.

---

### Story 23.3 — Paginated Photo Library API

As a user, I want the Photos tab to page through every image in Strife in stable capture-time order so that a large library can be browsed without loading every row. **Estimated: 8 points.**

**Acceptance Criteria:**

- [ ] `GET /api/photos` returns active finalized image nodes across the whole library and supports optional `folder_id`, recursive-folder, and `favorite` scopes without changing the definition of a photo; Story 25.2 adds `album_id` to the same contract.
- [ ] Each item includes node ID, parent ID, filename, byte size, capture time, dimensions, orientation, camera, lens, exposure fields, GPS availability and coordinates, processing status, favorite state, and URLs or identifiers needed for thumbnail, preview, and download behavior.
- [ ] The API does not return raw metadata JSON, storage keys, local paths, or any other implementation detail that bypasses existing file authorization and response rules.
- [ ] Ordering is `capture_time DESC NULLS LAST`, followed by source/created time and UUID as deterministic tie-breakers; a documented null-time bucket prevents undated photos from disappearing between pages.
- [ ] Pagination is keyset-based, uses an opaque validated cursor, defaults to 200 photos, and enforces a hard upper bound.
- [ ] The response includes total count and byte size for the selected scope plus bounded month buckets with opaque jump anchors, so navigating to an unloaded month does not require fetching every preceding page.
- [ ] Trash is excluded by default; the endpoint never leaks deleted nodes or unfinished uploads.
- [ ] Checked SQLx is used where the result shape is statically expressible; any runtime query exception is added to the generated inventory with a specific reason.
- [ ] PostgreSQL integration tests cover MIME/metadata inclusion, trash exclusion, every scope, equal and missing timestamps, cursor stability, and permanent deletion between pages.

---

### Story 23.4 — Photo Folder and Album Navigation API

As a user, I want a compact source tree with image counts so that I can move between the complete library, folders, favorites, and albums without walking the general file browser. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `GET /api/photos/tree` returns only active folders containing image descendants, with stable parent relationships, direct-photo counts, recursive-photo counts, and aggregate bytes.
- [ ] The response includes synthetic roots for All Photos and Favorites plus album summaries once Epic 25 lands; it does not duplicate Trash in the Photos MVP.
- [ ] Folder counts use the same photo eligibility rule as `GET /api/photos`, including finalized objects, normalized image kind, and active lifecycle state.
- [ ] Large trees load one hierarchy level at a time with pagination and a server-side cap, following the lazy-tree pattern established by the OCR Documents view.
- [ ] Counts remain correct when a photo is moved, trashed, restored, permanently deleted, or reclassified by metadata extraction.
- [ ] The endpoint exposes UUIDs and labels only, never filesystem storage keys or watched-folder source paths.
- [ ] API integration tests prove nested counts, empty-folder exclusion, pagination, lifecycle changes, and agreement with the corresponding photo-list scope.

---

### Story 23.5 — Grid-Safe Thumbnail and Preview Delivery

As a user, I want photos to appear progressively without launching an unbounded preview workload so that opening the tab stays safe on Orion. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] The Photos client uses the existing thumbnail endpoint for grid cells, preview endpoint for the inspector/lightbox, and download endpoint for originals.
- [ ] Virtualization bounds concurrent thumbnail requests; the client does not request media for every item returned by a page before it is near the viewport.
- [ ] Repeated requests for a missing thumbnail reuse one active preview-generation job per node and never create a job stampede.
- [ ] Ready immutable artifacts receive cache validators or an equivalent cache policy; pending and failed responses are not cached as permanent successes.
- [ ] Browser-native images and RAW files follow the existing preview decision rather than adding Photos-only decoder behavior.
- [ ] Broken, unsupported, pending, and failed media render stable placeholders with retry/details affordances and no broken-image chrome.
- [ ] Tests cover ready, queued, failed, unsupported, and concurrent-request behavior without weakening the resource-class and foreground-priority guarantees used by backfill work.

---

## Epic 24 — Photos Tab and fstop Library Experience

**Goal:** A dedicated Photos tab adapts fstop's fast three-pane album/library experience to Strife's navigation, visual system, file actions, and accessibility contract.

**Sprint Capacity Estimate:** 1–2 sprints

---

### Story 24.1 — Photos Route, Navigation, and Static Preview

As a user, I want a Photos destination in the Strife sidebar so that image browsing is separate from the general file table. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] The primary sidebar contains one Photos entry with an icon consistent with the established icon sprite and active-route behavior.
- [ ] `/photos` opens All Photos; folder and album selection are represented in route state or URL query parameters so reload and browser history preserve the current view.
- [ ] The Photos route stays inside the existing Strife shell, theme provider, storage summary, toast system, and error contract.
- [ ] Static preview mode contains deterministic folders, albums, dates, JPEG, PNG, and RAW examples so the complete layout can be reviewed without an API.
- [ ] Loading, empty-library, empty-scope, recoverable-error, and retry states are designed rather than falling back to a blank grid.
- [ ] A route-level test verifies sidebar navigation, deep-link restoration, and static-preview rendering.

---

### Story 24.2 — Three-Pane Photo Library Shell

As a user, I want fstop's source rail, photo grid, and always-available inspector adapted to Strife so that browsing context and photo details remain visible together. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Inside the existing Strife application shell, Photos renders a source rail, a flexible center grid, and a details inspector without introducing fstop's separate top-level shell or status bar.
- [ ] The source rail lazily expands Strife folders and lists All Photos, Favorites, and Albums with counts from the Photos APIs.
- [ ] Selecting a source replaces the photo page atomically, resets invalid selection, preserves the current source in the URL, and exposes an accessible current-state indication.
- [ ] Pane widths, borders, typography, focus styles, and colors use Strife tokens in both light and dark themes rather than copying fstop's dark-only constants.
- [ ] The grid remains the primary flexible pane and neither rail can force horizontal page overflow at supported desktop widths.
- [ ] Source loading, empty, partial-error, retry, and pagination states are visible without blocking already-loaded photos.
- [ ] Component tests verify source expansion, view switching, URL state, error recovery, and selection reset.

---

### Story 24.3 — Virtualized Photo Grid and Month Navigation

As a user, I want a dense, smooth photo grid with capture-month navigation so that tens of thousands of Strife images remain practical to browse. **Estimated: 8 points.**

**Acceptance Criteria:**

- [ ] The implementation adapts fstop's row-virtualized square grid and uses a maintained Solid-compatible virtualizer; only visible and bounded overscan rows exist in the DOM.
- [ ] Column count responds to available center-pane width while maintaining square cells, a stable small gap, and no layout shift when thumbnails load.
- [ ] Infinite loading uses the API cursor, suppresses duplicate requests, retains already-loaded cells during failures, and provides an explicit retry at the failed boundary.
- [ ] A capture-month jump list is derived from the API month buckets, marks the active month as the user scrolls, and loads intervening pages safely when the target month is not yet present.
- [ ] Image orientation is respected by the generated thumbnail; the client uses `object-fit: cover` without reapplying EXIF rotation.
- [ ] Pending and failed thumbnails use accessible placeholders, and each image has an informative accessible name even when visual captions are hidden.
- [ ] Tests cover responsive column calculation, virtual row bounds, cursor pagination, month jumping, placeholder replacement, and a failed next-page retry.

---

### Story 24.4 — Keyboard Cursor and Multi-Selection

As a user, I want fstop-style keyboard navigation plus familiar pointer selection so that large photo sets can be reviewed efficiently and accessibly. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Arrow keys move a roving grid cursor by cell or row and keep it visible; Home, End, Page Up, and Page Down have documented behavior.
- [ ] Enter or double-click opens the selected photo; Escape closes overlays or clears the current selection according to one documented priority order.
- [ ] Click selects one photo, Ctrl/Cmd-click toggles membership, and Shift-click extends a contiguous range without losing keyboard focus.
- [ ] Selection is keyed by node UUID rather than array index so loading another page cannot silently move selected state to another photo.
- [ ] The current cursor and selected cells have distinct token-driven visual states and correct `aria-selected`, focus, and grid semantics.
- [ ] Keyboard handlers ignore editable controls and do not conflict with Strife's command bar, dialogs, or preview modal.
- [ ] Tests cover row-boundary movement, incomplete final rows, selection modifiers, pagination, source changes, editable controls, and screen-reader state.

---

### Story 24.5 — Photo Inspector

As a user, I want the selected photo's preview and EXIF details beside the grid so that I can review images without opening a modal for each one. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] The inspector receives the selected photo's full list projection and does not issue one details request for every cursor move under normal browsing.
- [ ] It shows filename, position, preview, dimensions, size, capture time, camera, lens, exposure, ISO, focal length, orientation, favorite state, processing state, and GPS only when present.
- [ ] Missing metadata is represented consistently and never renders the strings `null`, `undefined`, or invalid dates.
- [ ] The histogram area renders normalized bins when Story 23.2 stores them and otherwise uses the ADR-approved empty state.
- [ ] Actions reuse Strife behavior for favorite/unfavorite, open preview, download, move, trash, restore where applicable, and open the full existing file-details panel.
- [ ] Changing selection aborts or ignores stale preview requests, preloads only bounded adjacent media, and never shows the previous photo under the new filename.
- [ ] Component tests cover complete, partial, pending, failed, GPS, RAW, and stale-request states.

---

### Story 24.6 — Photo Lightbox and Sequential Review

As a user, I want a full-screen photo viewer with previous and next navigation so that I can review an album without returning to the grid after every image. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] The implementation adapts fstop's lightbox interaction but reuses or extends Strife's existing preview modal instead of shipping two conflicting full-screen preview systems.
- [ ] The viewer displays the generated preview or browser-native original according to Strife's existing preview rules and never exposes a storage path.
- [ ] Previous and next controls support buttons, Arrow Left/Right, documented optional h/l shortcuts, and bounded neighbor preloading.
- [ ] Navigation crosses loaded-page boundaries by requesting the next cursor page without skipping or duplicating a photo.
- [ ] The viewer shows filename, position, dimensions, size, camera/exposure summary, download, favorite, and details controls.
- [ ] Focus is trapped while open, the invoking grid cell regains focus on close, Escape closes, backdrop behavior is documented, and reduced-motion preferences are honored.
- [ ] Tests cover first/last items, next-page loading, keyboard and pointer controls, pending/failed previews, focus restoration, and browser history behavior.

---

### Story 24.7 — Responsive and Accessible Photos Experience

As a user on a smaller screen or assistive technology, I want the Photos tab to preserve every core task even when the desktop three-pane layout cannot fit. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] At a documented breakpoint, the source rail becomes a drawer or selector and the inspector becomes a drawer/sheet while the grid remains usable.
- [ ] Touch targets, zoom, scroll, and lightbox dismissal work on representative phone and tablet viewports without hover-only controls.
- [ ] The page has one logical heading structure, named navigation regions, grid semantics, announced loading/error states, and no keyboard trap outside the modal.
- [ ] Both themes meet WCAG AA contrast for text, focus, selection, placeholder, and disabled states.
- [ ] A focused accessibility test uses axe-core and interaction tests cover keyboard-only and representative touch-width flows.
- [ ] Browser checks cover the supported desktop, tablet, and mobile widths in both themes with no horizontal document overflow or console errors.

---

## Epic 25 — Virtual Albums and Photo Organization

**Goal:** Users can organize Strife photos into named virtual albums without changing folder hierarchy, duplicating bytes, or weakening node lifecycle rules.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 25.1 — Album and Membership Schema

As a user, I want durable albums linked to Strife photo nodes so that one photo can appear in several collections without being copied. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] A reversible migration adds `photo_albums` and `photo_album_items` with UUID primary keys, timestamps, stable item position, and foreign keys to nodes.
- [ ] Album names are trimmed, non-empty, length-bounded, and unique under the product's current single-library ownership model.
- [ ] Membership is unique per album and node, and only finalized image files may be added through the database/API contract.
- [ ] Permanently deleting a node cascades its memberships; trashing it retains membership but hides it from the normal album view until restored.
- [ ] Deleting an album removes memberships only and never moves, trashes, or deletes photo nodes or file objects.
- [ ] Album item ordering has a deterministic default based on capture time and supports a future explicit manual order without a destructive migration.
- [ ] PostgreSQL integration tests cover constraints, duplicate membership, trash/restore visibility, node deletion, and album deletion.

---

### Story 25.2 — Album CRUD and Membership API

As a user, I want to create albums and add or remove selected photos so that the Photos tab supports meaningful personal collections. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] REST endpoints list, create, rename, and delete albums and add or remove one or many node IDs from an album.
- [ ] Batch membership mutation is transactional, idempotent, bounded, and returns per-request totals without partial success hidden behind HTTP 200.
- [ ] Every mutation validates active finalized image nodes and rejects folders, non-images, unfinished uploads, and deleted nodes with the unified API error shape.
- [ ] Album list responses contain item count, optional cover photo ID, and updated time without fetching full member lists.
- [ ] `GET /api/photos?album_id={id}` uses the ordering, cursor, projection, month buckets, and lifecycle filtering established by Story 23.3.
- [ ] Album deletion requires explicit confirmation in the UI contract and remains non-destructive to files.
- [ ] Concurrent duplicate additions cannot create duplicate memberships, and rename collisions return a conflict rather than silently changing another album.
- [ ] Route ownership and PostgreSQL API integration tests cover all success, validation, idempotency, conflict, and lifecycle cases.

---

### Story 25.3 — Album Navigation and Management UI

As a user, I want albums in the Photos source rail with straightforward management controls so that collections are easy to enter and maintain. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] The source rail lists albums with counts, lazy-loads or paginates when needed, and opens an album through stable URL state.
- [ ] Users can create, rename, and delete albums through accessible dialogs that follow Strife's existing validation, confirmation, toast, and error conventions.
- [ ] Creating an album can optionally add the current photo selection in the same user flow without creating partial hidden state.
- [ ] Deleting an album explains that files remain in Strife and returns the user to All Photos if the deleted album was open.
- [ ] Empty albums remain visible and show a useful empty state with an add-photos action.
- [ ] Static preview mode includes empty and populated albums.
- [ ] Component tests cover CRUD, URL updates, optimistic or pessimistic refresh behavior, conflicts, cancellation, and deletion of the active album.

---

### Story 25.4 — Add and Remove Photos from Albums

As a user, I want album actions on one or many selected photos so that organizing a large shoot does not require opening each file individually. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] The grid selection toolbar and inspector provide Add to Album; an album view additionally provides Remove from Album.
- [ ] The picker supports adding the selection to one or multiple existing albums and creating a new album without discarding selection.
- [ ] Duplicate additions are treated as successful no-ops and the result reports added versus already-present counts.
- [ ] Removing from an album never trashes, moves, unfavorites, or deletes a node and immediately removes it from the active album view.
- [ ] Actions remain bounded for large selections and display progress or a clear limit rather than issuing one request per photo.
- [ ] Tests cover mixed existing/new membership, multi-album addition, removal, API failure, selection retention, and keyboard operation.

---

## Epic 26 — Photos Scale, Quality, and Production Readiness

**Goal:** The Photos tab is proven against Strife's production-shaped library, remains responsive on Orion, and is safe to deploy without triggering uncontrolled derived-media work.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 26.1 — End-to-End Photos Workflow Coverage

As a maintainer, I want production-shaped tests from import through album browsing so that the feature does not depend on hand-built database state. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] An end-to-end test imports a nested synthetic set containing JPEG, PNG, RAW, a misleading extension, a non-image, missing EXIF, equal capture times, and one extractor failure.
- [ ] The test waits for metadata and required thumbnail work through public status contracts, then verifies All Photos, folder counts, favorites, month buckets, and album membership.
- [ ] The browser flow opens Photos, changes sources, scrolls through a cursor boundary, selects a photo, verifies inspector data, opens the lightbox, and navigates across a page boundary.
- [ ] Trash, restore, move, favorite, album add/remove, and permanent deletion update every relevant photo view without stale or duplicate entries.
- [ ] Re-running metadata and preview generation is idempotent and does not duplicate artifacts or album membership.
- [ ] The suite asserts no uncaught browser errors and verifies the unified API error contract on at least one failed media request.

---

### Story 26.2 — Production-Scale Query and Grid Benchmark

As a maintainer, I want measured photo browsing at production scale so that the new tab remains useful against the real Strife archive. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] A deterministic generator creates at least 100,000 photo projections across deep folders, years, months, missing dates, favorites, albums, and trash without requiring matching large image blobs.
- [ ] Benchmarks measure All Photos first page, folder tree, deep-folder page, album page, cursor continuation, and month-bucket queries with warm and cold PostgreSQL caches.
- [ ] Initial targets are documented before measurement: p95 under 150 ms for a 200-item photo page and under 100 ms for one tree level on Orion-class hardware, or measured exceptions receive an explicit follow-up.
- [ ] `EXPLAIN (ANALYZE, BUFFERS)` output demonstrates index-backed ordering and scope filters without full-table sorts on normal page requests.
- [ ] Browser profiling verifies bounded DOM nodes, bounded concurrent media requests, no unbounded retained photo pages, and smooth representative scrolling under CPU throttling.
- [ ] Required indexes and any denormalized count strategy are justified by measurements and covered by migration tests rather than added speculatively.

---

### Story 26.3 — Deployment and Derived-Media Safety

As an operator, I want the Photos tab deployable independently from historical thumbnail generation so that enabling navigation does not monopolize Orion. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Database migrations are additive and compatible with the previously deployed API, worker, and web images during a rolling Compose update.
- [ ] Deploying the Photos API or web route does not automatically enqueue thumbnails or previews for the historical library.
- [ ] On-demand media work retains foreground priority and existing resource-class limits; opening one grid cannot claim more concurrent CPU-heavy work than configured.
- [ ] A read-only preflight reports eligible photos, metadata gaps, thumbnail readiness, preview readiness, failed artifacts, and estimated incremental storage without enqueueing work.
- [ ] The Orion runbook covers migration-first deployment, API/readiness checks, Photos smoke tests, resource observation, rollback, and the fact that additive album data remains after an image rollback.
- [ ] A staged validation opens small folder and album scopes before All Photos and records API latency, worker concurrency, CPU, memory, storage growth, and failures.
- [ ] Rollback to the previous web/API image leaves existing Strife browsing, imports, metadata, previews, OCR, and email behavior intact.

---

### Story 26.4 — Documentation and fstop Reuse Reconciliation

As a maintainer, I want the shipped Photos behavior and its fstop provenance documented so that future changes preserve the intended boundary. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `README.md`, setup documentation, known limitations, supported formats, and the Photos ADR agree on supported media, album semantics, preview behavior, and deferred features.
- [ ] The implementation identifies which fstop components and interaction patterns were adapted and records any source-license or attribution obligations before code is copied.
- [ ] User documentation explains All Photos, folders, favorites, albums, selection, keyboard navigation, inspector fields, lightbox controls, and missing/failed metadata states.
- [ ] Operator documentation explains that the tab reads existing Strife files, does not import from fstop, and does not launch a historical thumbnail backfill on deployment.
- [ ] Static-preview fixtures, screenshots, and accessibility checks are updated to represent the final responsive behavior in both themes.
- [ ] `docs/scrum/index.md` and any authoritative split-story mirrors are generated or updated according to the repository's Scrum documentation convention.

---

## Summary

| Epic                                                 | Milestone | Stories | Estimated Points |
| ---------------------------------------------------- | --------- | ------- | ---------------- |
| 23 — Photo Projection and Browse APIs                | M23       | 5       | 24               |
| 24 — Photos Tab and fstop Library Experience         | M24       | 7       | 36               |
| 25 — Virtual Albums and Photo Organization           | M25       | 4       | 18               |
| 26 — Photos Scale, Quality, and Production Readiness | M26       | 4       | 18               |
| **Photos Total**                                     |           | **20**  | **96**           |

> [!TIP]
> At the roughly 30 points per sprint assumed in [`scrum.md`](../scrum.md), this is approximately **3–4 sprints**. A useful first vertical slice is Stories 23.1, 23.3, 23.5, 24.1, 24.2, 24.3, 24.5, and 24.6: All Photos with a real Strife query, safe thumbnails, the three-pane shell, inspector, and lightbox before album mutations are introduced.

> [!IMPORTANT]
> Suggested order: settle Story 23.1 first, then build the list and tree contracts before the UI. Story 23.2 can proceed beside those APIs. Epic 24 should prove All Photos and folder browsing before Epic 25 adds mutation state. Epic 26 gates production deployment; in particular, the web route must not be treated as permission to enqueue derived media for the whole historical library.

> [!NOTE]
> The visual reference is fstop, but the product behavior is Strife. Reuse the fast grid, keyboard review, source navigation, inspector, and lightbox ideas while preserving Strife's global shell, UUID node identity, themes, API error contract, lifecycle rules, file actions, artifact storage, and operational limits.
