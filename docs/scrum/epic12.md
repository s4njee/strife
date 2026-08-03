# Epic 12 — Structural Maintainability


**Goal:** The files that concentrate the most change are split along the seams that v2 work will follow.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 12.1 — Decompose the Data Access Layer

As a developer, I want the data access layer split by domain so that v2 features do not all edit the same file. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `crates/db/src/lib.rs` (3,176 lines) is split into modules along existing seams: nodes and hierarchy, file objects, upload sessions, jobs, metadata, artifacts, trash, favorites, and import entries.
- [ ] `lib.rs` retains only module declarations, shared types, and `MIGRATOR`.
- [ ] Public paths are preserved by re-export so that no caller in `crates/api`, `crates/worker`, or `crates/importer` changes.
- [ ] The split is a pure move: no query text or behavior changes in the same commit, so the diff stays reviewable.
- [ ] Inline test modules move alongside the code they cover.
- [ ] The existing `crates/db/tests` files pass unchanged.

---

### Story 12.2 — Decompose the Workspace View & API Client

As a developer, I want the largest frontend files split into focused units so that UI changes stay local. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `apps/web/src/views/WorkspaceView.tsx` (1,005 lines) is split so that the browse, trash, and favorites views — each with its own `createResource` and `refetch` — become separate components or one shared parameterized list view.
- [ ] Drag-and-drop handling, selection, and context-menu wiring are extracted from the view body.
- [ ] `apps/web/src/api/client.ts` (1,023 lines) is split by resource (folders, nodes, files, uploads, imports, storage) behind the same import surface.
- [ ] Behavior is unchanged and verified against the tests added in Epic 11.
- [ ] `eslint --max-warnings 0` and `prettier --check` pass.
- [ ] The upload-flow file reference in `notes.md` is updated to match the new layout.

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
| **v1 Total** | | **87** | **284** |
| 8 — Observability & API Error Contract | M8 | 5 | 22 |
| 9 — Production Deployment & Process Lifecycle | M9 | 4 | 10 |
| 10 — Queue Durability & Configuration Hygiene | M10 | 3 | 8 |
| 11 — Frontend Test Foundation & API Contract | M11 | 4 | 19 |
| 12 — Structural Maintainability | M12 | 2 | 10 |
| **Pre-v2 Hardening Total** | | **18** | **69** |
| **Total** | | **105** | **353** |

> [!TIP]
> At a velocity of ~30 points/sprint with 2-week sprints, v1 is approximately **10 sprints (~20 weeks)** of work and the pre-v2 hardening epics add roughly **2–3 sprints**. Adjust based on measured velocity after the first 2 sprints.

> [!IMPORTANT]
> Suggested order for the hardening epics: **8.2 and 8.3 first** — until internal errors are logged, every other change in this list is harder to verify in production. **9.1 next**, because the deployed configuration is currently untracked and one bug fix is uncommitted. Epic 10 should land before any v2 feature adds a third job type per file.
