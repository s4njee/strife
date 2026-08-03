# Epic 11 — Frontend Test Foundation & API Contract


**Goal:** The frontend has automated tests for its riskiest logic, and API types stop being maintained by hand on both sides.

**Sprint Capacity Estimate:** 1–2 sprints

---

### Story 11.1 — Frontend Test Harness

As a developer, I want a test runner in the frontend so that frontend logic can be tested at all. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] A test runner (Vitest, matching the existing Vite toolchain) is added to `apps/web` with `test` and `test:watch` scripts in `package.json`.
- [ ] `@solidjs/testing-library` or an equivalent is configured so components can be rendered and asserted.
- [ ] `.github/workflows/ci.yml` runs the frontend test suite alongside the existing lint, format, and build steps.
- [ ] At least one test exists for a pure module (`apps/web/src/commands/parse.ts`) and one for a component, proving both paths work.
- [ ] Coverage reporting is configured and a baseline is recorded; no coverage threshold is enforced yet.
- [ ] `docs/development` documents how to run frontend tests.

---

### Story 11.2 — Upload Engine Unit Tests

As a developer, I want the chunked upload engine covered by tests so that resume logic can be changed safely. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `apps/web/src/uploads/folderUpload.ts` is tested for chunk boundary calculation, missing-range computation on resume, the three-way concurrency cap, and `ensureFolderPath` hierarchy creation.
- [ ] `apps/web/src/uploads/uploadPersistence.ts` is tested for storing, rehydrating, and clearing `File` handles in IndexedDB, including a rehydration attempt for a session the server no longer knows about.
- [ ] `apps/web/src/uploads/dropFiles.ts` is tested for recursive `FileSystemEntry` traversal, preserved relative paths, and empty directories.
- [ ] `apps/web/src/uploads/UploadContext.tsx` is tested for `AbortController` cancellation and for server session discovery.
- [ ] The resume path is covered end to end at unit level: a session with chunks 1–3 already received produces requests for the remaining ranges only.
- [ ] Tests run without a live API; `fetch` is stubbed.

---

### Story 11.3 — Generated or Contract-Tested API Types

As a developer, I want frontend types derived from the backend rather than hand-maintained so that API changes cannot silently diverge. **Estimated: 8 points.**

**Acceptance Criteria:**

- [ ] A decision is recorded resolving the open question in `deferred.md`: generate an OpenAPI document from the Axum handlers and generate TypeScript from it, generate TypeScript directly from the Rust types, or keep them separate behind an enforced contract test.
- [ ] `apps/web/src/api/types.ts` is generated or contract-verified rather than maintained by hand.
- [ ] The runtime type guards in `apps/web/src/api/client.ts` are retained — they guard against a misbehaving server — but are derived from or checked against the same source of truth.
- [ ] CI fails when a backend response type changes without a corresponding frontend update.
- [ ] The shared `ErrorBody` shape from Story 8.1 is part of the generated contract.
- [ ] Error responses surface the server's `message` where it is useful, instead of `client.ts` discarding it in favour of strings such as `Could not favorite item (500).`

---

### Story 11.4 — Background Job Completion Refresh

As a user, I want the file list to update when background processing finishes so that metadata and previews appear without a manual refresh. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] A decision is recorded on the mechanism: extend the existing polling used by `StatusFooter`, `StorageWarning`, and `ImportStatusView`, or introduce server-sent events.
- [ ] After an upload finalizes, the workspace reflects metadata and preview availability without the user navigating away and back; `WorkspaceView` currently has no polling of its own.
- [ ] Refresh is scoped to the visible folder and stops when the view is unmounted or the tab is hidden.
- [ ] The existing per-component intervals are reconciled so the client does not issue several uncoordinated polls per 15-second window.
- [ ] The fixed 750 ms preview poll in `prepareFilePreview` is reconsidered against the chosen mechanism.
- [ ] Test: a completed job causes exactly one refetch of the affected folder.

---
