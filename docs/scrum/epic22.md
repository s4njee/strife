# Epic 22 — Backfill Operations, Security & Production Readiness


**Goal:** The ten-year archive can be indexed safely, observed in real time, resumed after interruption, upgraded, and validated at production scale.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 22.1 — Email Status API & SSE Backfill Console

As a user, I want live extraction progress so that a long initial archive backfill is observable without page refreshes or polling. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `GET /api/email/status` reports candidate, pending, running, completed, failed, skipped, unsupported, remaining, attachment-processing, parser version, and indexed-message counts using aggregate SQL.
- [ ] Status separates foreground jobs from each historical campaign and reports campaign state, durable cursor, configured batch/queue/running limits, throughput, and estimated completion time.
- [ ] `GET /api/email/events` streams `entry` and `status` server-sent events with the same keep-alive, cursor-resume, and disconnect-resource guarantees as the import/OCR streams.
- [ ] Events include node, filename, extracted subject when safe, state, attachment count, duration, and concise warning; they never include body text or sensitive raw headers.
- [ ] The Email page includes a bounded newest-first processing console and connection state.
- [ ] Failed messages support per-file retry and confirmed bounded bulk retry.
- [ ] Authorized campaign controls start, pause, resume, and cancel; start requires a completed preflight and explicit confirmation of candidate count and resource policy.
- [ ] Repeated connection/disconnection tests prove database connections are released.
- [ ] Empty, mixed, active-backfill, all-complete, and parser-version-mismatch status tests use isolated PostgreSQL databases.

---

### Story 22.2 — Parser Resource Limits & Failure Isolation

As an operator, I want hostile or pathological email bounded so that one message cannot stop the archive backfill. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Configurable limits cover source bytes, MIME parts, header bytes/count, nesting depth, decoded body bytes, decoded attachment bytes, attachments per message, total decoded bytes, parser time, and stored warnings.
- [ ] Defaults are documented as provisional and profiled on Orion before being treated as final.
- [ ] Limit failures name the exact limit, persist a safe warning, and become terminal when retry cannot change the outcome.
- [ ] Parser work is isolated consistently with other extractors; any remaining memory/process isolation gap is recorded in `docs/known-limitations.md`.
- [ ] Worker concurrency for email and attachment extraction is separately configurable, and OCR/email/attachment backfills acquire the shared `HEAVY_CPU` admission permit before claiming work.
- [ ] `OCR_CONCURRENCY`, `EMAIL_PARSE_CONCURRENCY`, `ATTACHMENT_EXTRACTION_CONCURRENCY`, and `HEAVY_CPU_CONCURRENCY` default to 1 on Orion; the shared permit is enforced across worker processes through the renewable `worker_resource_leases` slots created by `0017_backfill_campaigns`, not through an advisory lock held for the duration of a parse.
- [ ] Email body parsing starts under `heavy_cpu` admission and moves to `extractor` only after the 10,000-message canary in Story 22.5 records safe resource behavior, per the resource-class table in [`backfill.md`](../backfill.md); attachment extraction and every Tesseract path stay `heavy_cpu` regardless. The promotion is a recorded, reversible configuration change with a named owner, not an undocumented code edit.
- [ ] Foreground jobs use higher priority than repair jobs, which use higher priority than historical jobs; a documented fairness budget lets backfill progress without starving new work.
- [ ] Backfill worker containers have explicit CPU and memory ceilings in Compose; application concurrency is not treated as a substitute for a container CPU quota.
- [ ] Structured logs contain message/node/job identifiers and measurements but no body, subject, address, raw header, or attachment content.
- [ ] Synthetic tests cover every limit, cancellation, worker restart, lease expiry, malformed recursive MIME, and cleanup.

---

### Story 22.3 — Email Privacy & Rendering Threat Model

As an operator, I want archived email treated as hostile sensitive content so that search and rendering do not create a new exfiltration surface. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `docs/security/email-threat-model.md` covers malicious MIME, parser vulnerabilities, decompression expansion, HTML/script injection, CSS leaks, tracking pixels, remote resources, unsafe URLs, attachment MIME spoofing, header injection, sensitive logs, and search-snippet leakage.
- [ ] Worker/container network policy prevents email parsing and attachment extraction from making outbound requests where deployment capabilities allow it.
- [ ] HTML sanitizer configuration is pinned and regression-tested against the threat-model fixture set.
- [ ] API responses set appropriate content types and defensive headers; raw email and unsafe attachments never render inline in the Strife application origin.
- [ ] Search and event logs redact query/body/address values by default while retaining operational correlation IDs.
- [ ] Security dependency updates trigger the relevant parser, sanitizer, MIME, and attachment regression suites.

---

### Story 22.4 — Versioning, Repair & Operational Controls

As an operator, I want parser/index versions and repair tools exposed so that upgrades and interrupted backfills are manageable. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] Current parser, sanitizer, normalization, attachment-extractor, and search-index versions are recorded independently where their changes require different reprocessing.
- [ ] An admin status response reports version distributions and counts requiring reprocessing.
- [ ] Bounded repair commands detect missing projections, orphan artifacts, attachment manifest/artifact mismatches, stale active jobs, and index-version mismatches without mutating state in dry-run mode.
- [ ] Mutating repair/reprocess actions require explicit scope and batch limits and report exactly what changed.
- [ ] Campaign repair detects cursor/count drift and active-job mismatches, and it cannot convert a paused campaign to running as a side effect.
- [ ] Restarting workers during backfill resumes leases and does not duplicate messages, addresses, labels, attachments, or events.
- [ ] Runbooks defer to [`backfill.md`](../backfill.md) and document initial preflight, foreground-only deployment, canary stages, pause/resume, parser upgrade, index rebuild, failure triage, and image rollback.

---

### Story 22.5 — End-to-End Archive Validation

As a maintainer, I want production-shaped validation from import through search and reading so that the feature is trustworthy before indexing the only historical archive. **Estimated: 8 points.**

**Acceptance Criteria:**

- [ ] An end-to-end test imports a directory tree of synthetic `.eml` files through the watched-folder path and verifies automatic extraction, status events, weighted search, filters, snippets, reader data, attachments, duplicate collapse, threads, trash exclusion, and permanent-delete cascades.
- [ ] Direct upload of the same fixture produces equivalent parsed and searchable results.
- [ ] Restart tests interrupt parsing, attachment materialization, attachment text extraction, and index backfill, then verify idempotent recovery.
- [ ] A preflight command scans the real archive read-only and reports file count, total bytes, MIME confidence, size percentiles, malformed candidates, duplicate estimates, and projected database/artifact disk use without exposing message content.
- [ ] The rollout follows [`backfill.md`](../backfill.md): additive migration and foreground-only deploy first; then email canaries of 100, 1,000, and 10,000; then the full email body campaign; ordinary OCR only after email body indexing; attachment text and attachment OCR last.
- [ ] Each canary records throughput, p50/p95 duration, CPU, memory, temperature, I/O wait, database/index growth, failures, and estimated completion time before the next stage is authorized.
- [ ] The 10,000-message canary explicitly decides the email resource-class promotion from Story 22.2 — promote to `extractor` or hold at `heavy_cpu` — and records the measurement the decision rests on.
- [ ] PostgreSQL and managed-storage backup requirements are documented before full backfill; parsed projections may be rebuilt, but canonical `.eml` originals must be included in backup and restore drills.
- [ ] ARM64 Orion validation records throughput, CPU, memory, database growth, attachment growth, and the final resource-limit adjustments.
- [ ] Rust formatting, zero-warning Clippy, full workspace tests, frontend formatting/lint/build, migration rollback checks, and worker-image build all pass.

---

## Summary

| Epic                                                      | Milestone | Stories | Estimated Points |
| --------------------------------------------------------- | --------- | ------- | ---------------- |
| 17 — Email Decisions, Schema & Queue Foundation           | M17       | 4       | 16               |
| 18 — MIME Extraction & Durable Email Projection           | M18       | 6       | 31               |
| 19 — Email Full-Text Search & Query API                   | M19       | 5       | 23               |
| 20 — Email Navigation, Search & Reader UI                 | M20       | 5       | 20               |
| 21 — Attachment Search, Threads & Gmail Context           | M21       | 4       | 23               |
| 22 — Backfill Operations, Security & Production Readiness | M22       | 5       | 24               |
| **Email Total**                                           |           | **29**  | **137**          |

> [!TIP]
> At the roughly 30-points-per-sprint planning velocity used by [`scrum.md`](../../scrum.md), the complete plan is approximately **5 sprints**. A useful first release can stop after Stories 17.1–20.5: structured extraction, weighted body/header search, filters, and a safe reader. Epic 21 adds attachment content and richer archive context; Epic 22 is required before an unattended full-history production backfill.

> [!IMPORTANT]
> Suggested order: **17.1 and 17.2 first**, because parser output and schema form the compatibility contract. Build **17.4 before 18.1** so parser selection is evidence-based. Complete 18.5 before indexing. Stories 19.1–19.4 and 20.1–20.3 can then overlap. Do not render HTML before 20.4's security boundary exists, and do not launch the full ten-year backfill before Stories 22.1–22.5 are complete.

> [!NOTE]
> Shipping the feature and starting the historical backfill are deliberately separate operations. Production deployment uses foreground processing only with every campaign paused. The operator follows [`backfill.md`](../backfill.md) after health checks, backup validation, and a read-only archive preflight.

> [!WARNING]
> Four choices remain intentionally measurement-driven: whether quoted reply/signature stripping improves ranking, the final parser and attachment resource limits on Orion, the corpus size at which PostgreSQL stops meeting latency targets, and whether attachment artifacts should be backed up or regenerated. The stories above require measurements and explicit documentation before changing the initial decisions.
