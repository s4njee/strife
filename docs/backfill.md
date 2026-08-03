# Strife Historical Processing & Backfill Plan

This document defines how Strife will deploy OCR and email-search support into a library of roughly 700,000 existing files without flooding PostgreSQL, saturating Orion's CPU, starving interactive work, or starting historical processing merely because a new image was deployed.

It is the shared operational contract for [OCR](ocr.md) and [email archive search](email.md). Feature deployment and historical backfill are deliberately separate operations.

## Executive decision

Strife will:

1. Deploy additive schema and application support with historical campaigns disabled.
2. Continue processing newly finalized files as high-priority foreground work.
3. Inventory historical candidates through a read-only preflight.
4. Start one historical campaign explicitly, initially email header/body parsing.
5. Refill only a small bounded job window rather than enqueueing the full library.
6. Admit expensive OCR/email/attachment work through a shared renewable CPU-resource lease.
7. Run 100-, 1,000-, and 10,000-file canaries before a full campaign.
8. Complete email body indexing before ordinary historical OCR; process attachment text and attachment OCR last.
9. Pause automatically or manually when resource or health gates fail.

No migration, startup hook, recovery task, or ordinary deployment may enqueue the historical library.

## Why the current controls are insufficient

The production Compose configuration currently sets `WORKER_CONCURRENCY=1`, so one worker process cannot run two jobs simultaneously. That prevents immediate OCR/email overlap in today's single-worker topology, but it is not a complete backfill design:

- A repeated reprocess call could still create a very large durable backlog.
- One global processor cannot distinguish interactive work from historical work.
- Fixed job-family claim order can starve later families behind a continuously populated earlier family.
- In-process semaphores do not coordinate multiple worker containers.
- The worker container has a memory limit but no explicit CPU quota.
- The API and worker both apply migrations during startup, which is unsafe for a migration that rewrites or indexes hundreds of thousands of rows.
- A million pending job rows would make pause/cancel, observability, upgrades, and queue maintenance harder without increasing useful throughput.

`WORKER_CONCURRENCY=1` remains the initial safety setting, but campaign admission, origin-aware priority, shared resource leases, and container quotas are the durable solution.

## Safety invariants

The implementation must preserve these invariants:

- Existing canonical files are never modified or deleted by a backfill.
- Deploying support creates zero historical OCR/email jobs.
- New uploads/imports remain usable while a campaign is paused or running.
- Foreground ingestion, deletion, metadata, and preview work outranks historical processing.
- At most one `HEAVY_CPU` operation runs on Orion initially, across all worker processes.
- A campaign never has more queued or running work than its recorded limits.
- Pausing stops refills and new campaign claims; it does not kill a healthy leased child process midway through a file.
- Resuming continues from a durable cursor and does not rescan completed candidates.
- Cancelling does not delete completed projections or original files.
- Worker restart, API restart, image rollback, or lease expiry cannot duplicate projections.
- Logs/events never contain email bodies, addresses, subjects, raw headers, OCR text, or attachment contents.
- Every change to concurrency or resource limits is explicit, recorded, and reversible.

## Work classification

Every job gains two dimensions beyond job type.

### Origin

| Origin       | Meaning                                      | Initial priority |
| ------------ | -------------------------------------------- | ---------------- |
| `foreground` | New upload/import or user-requested action   | `+100`           |
| `repair`     | Failed/missing/version-specific reprocessing | `0`              |
| `backfill`   | Historical campaign work                     | `-100`           |

These values are defaults, not literals scattered through handlers. Claiming orders by origin class, job priority, creation time, and stable ID.

Foreground always wins, but a configurable fairness budget permits one backfill claim after a default of 20 foreground claims when interactive queue health remains acceptable. This prevents permanent historical starvation without allowing backfill to hide new work.

### Resource class

| Resource class | Examples                                      | Initial Orion capacity |
| -------------- | --------------------------------------------- | ---------------------- |
| `light`        | MIME detection, database-only repair          | worker concurrency     |
| `extractor`    | MIME email parsing, ExifTool, ffprobe, Tika   | 1                      |
| `preview`      | thumbnails and document previews              | 1                      |
| `heavy_cpu`    | Tesseract, PDF rasterization, attachment OCR  | 1 shared               |
| `heavy_io`     | attachment materialization, large index batch | 1                      |

Email body parsing begins conservatively under the campaign's shared heavy admission even though it is expected to be cheaper than OCR. After the 10,000-message canary proves safe resource behavior, it may move to `extractor` while attachment extraction and every Tesseract path remain `heavy_cpu`.

## Database changes

### Shared migration order

The shared foundation must land before email migrations:

1. `0017_backfill_campaigns`
2. `0018_email_messages`
3. `0019_email_job_type`
4. `0020_email_search`

If another feature consumes these numbers first, use the next available sequence and update both plans; dependency order matters more than the literal number.

### `backfill_campaigns`

The campaign table should contain at least:

```text
id
kind                    email | ocr | attachment_text | attachment_ocr
state                   draft | paused | running | draining | completed | cancelled | failed
candidate_definition    versioned JSON criteria, not executable SQL
snapshot_before         fixed upper boundary for a stable candidate set
cursor_created_at
cursor_node_id          tuple cursor with created_at
batch_size
max_queued
max_running
resource_class
foreground_fairness
candidate_count
enqueued_count
completed_count
failed_count
skipped_count
created_by_version
started_at
paused_at
completed_at
created_at
updated_at
last_error
```

Candidate definitions are validated typed structures. Arbitrary stored SQL is prohibited. A campaign freezes `snapshot_before` when it starts so files finalized later become foreground jobs rather than silently enlarging the historical campaign.

The cursor is `(nodes.created_at, nodes.id)`, not a UUID alone. Every candidate query uses the same deterministic ordering and advances the cursor in the transaction that enqueues its batch.

### Job changes

Add to `jobs`:

```text
origin          foreground | repair | backfill
campaign_id     nullable FK to backfill_campaigns
resource_class  light | extractor | preview | heavy_cpu | heavy_io
```

Constraints enforce that backfill jobs have a campaign and non-backfill jobs do not accidentally inherit one. Existing rows migrate as `foreground` without a table-wide application rewrite beyond the schema default/backfill procedure chosen for PostgreSQL.

The active uniqueness rule remains per job type and target. Campaign refill excludes active jobs before applying its batch limit.

### Resource leases

Cross-process admission uses database rows rather than a PostgreSQL advisory lock held for the duration of OCR. Holding an advisory lock would consume a pooled connection throughout a potentially long Tesseract run.

`worker_resource_leases` contains fixed slots per resource class:

```text
resource_class
slot_number
lease_owner
job_id
lease_expires_at
updated_at
PRIMARY KEY (resource_class, slot_number)
```

Workers atomically acquire a slot before claiming an eligible resource-class job. Resource and job leases renew together. Expired resource leases are recoverable. A worker that acquires a slot but finds no job releases it immediately.

The database capacity is authoritative across processes. Local semaphores remain useful to prevent excess child-process creation inside one worker, but they cannot raise the database capacity.

### Campaign events

Durable events record campaign state transitions, refills, health pauses, limit changes, and aggregate milestones. Per-file OCR/email events continue in their existing/specialized event tables and reference the campaign when applicable.

Events must record who/what initiated a transition, the old/new state, limits, counts, timestamp, and a safe reason. They must not contain extracted content.

## Campaign scheduler

The scheduler is a durable coordinator, not a loop that enumerates all candidates into memory.

For each running campaign:

1. Read current pending and leased jobs for the campaign using indexed counts.
2. If the active count is at or above `max_queued`, do nothing.
3. Calculate the refill allowance as the lesser of `batch_size` and remaining queue capacity.
4. Select eligible active nodes after the durable tuple cursor and before `snapshot_before`.
5. Exclude nodes with completed current-version output or active equivalent jobs.
6. Insert jobs and advance the cursor in one transaction.
7. Emit a refill/status event.
8. When no candidates remain and no jobs are active, mark the campaign complete.

Initial values:

```text
batch_size = 100
max_queued = 500
max_running = 1
heavy_cpu capacity = 1
```

The scheduler is disabled globally by default in production. `BACKFILL_ENABLED=true` allows already-running campaigns to refill; it does not start a draft or paused campaign.

### State behavior

- `draft`: preflight/candidate definition may change; no jobs.
- `paused`: candidate snapshot and cursor are durable; no refills or new claims.
- `running`: scheduler refills within limits and workers may claim.
- `draining`: no refills; already pending/leased work may finish.
- `completed`: cursor exhausted and no active work.
- `cancelled`: no refills/claims; pending campaign jobs transition to `skipped` with a campaign-cancelled reason; leased jobs drain.
- `failed`: coordinator failure requires inspection; workers do not claim new campaign jobs.

Starting a campaign requires explicit confirmation of its preflight count, candidate definition, limits, resource class, and snapshot boundary. Changing safety limits while running first pauses the campaign and records an event.

## Worker topology and configuration

### Phase-one topology

Keep the current single worker and global concurrency of 1 while proving campaign behavior. Add independent gates even though the global limit makes them appear redundant:

```text
OCR_CONCURRENCY=1
EMAIL_PARSE_CONCURRENCY=1
ATTACHMENT_EXTRACTION_CONCURRENCY=1
HEAVY_CPU_CONCURRENCY=1
BACKFILL_BATCH_SIZE=100
BACKFILL_MAX_QUEUED=500
BACKFILL_ENABLED=false
FOREGROUND_CLAIMS_PER_BACKFILL=20
```

### Phase-two topology

After canaries, split worker responsibilities without changing queue semantics:

- `worker-interactive`: foreground and repair origins; deletion, import, metadata, preview, new email, and new-file OCR.
- `worker-backfill`: backfill origin only; enabled only during an authorized campaign.

Both still use database resource leases, so foreground OCR and historical OCR cannot overlap merely because they run in separate containers.

Initial Compose limits should be conservative and finalized through Orion profiling:

```yaml
worker-interactive:
  cpus: "1.0"
  mem_limit: 1536m

worker-backfill:
  cpus: "0.75"
  mem_limit: 1536m
```

Do not start separate email and OCR backfill containers at the same time initially. One backfill worker is simpler and the campaign kind controls its eligible work.

### Health-based pause

Static concurrency is the primary guard. A running campaign may also transition to paused when monitored signals cross configured thresholds for a sustained interval:

- container OOM/restart;
- host thermal throttling;
- CPU saturation above the canary-approved ceiling;
- memory pressure or swap growth;
- excessive I/O wait;
- low free database/storage capacity;
- API readiness failures or unacceptable interactive latency;
- PostgreSQL connection, lock, replication/backup, or vacuum distress.

Automatic health pauses never automatically resume. An operator reviews the cause and resumes explicitly.

## Read-only preflight

Preflight runs before any campaign is created and writes no jobs or projections. It may persist only a bounded aggregate report after operator confirmation.

### Email preflight

Report:

- candidate count and total bytes;
- extension/provided-MIME/detected-MIME agreement;
- size percentiles and largest candidates;
- synthetic sample parse success/warning/failure rates;
- estimated duplicate rate without exposing identifiers;
- attachment count/decoded-size estimates from a bounded sample;
- projected PostgreSQL text/index and attachment-artifact growth;
- estimated duration using measured canary throughput.

### OCR preflight

Report:

- candidates by PDF, JPEG, PNG, TIFF, WebP, and RAW family;
- current embedded/OCR/failed/unsupported/missing text states;
- file-size percentiles and discoverable PDF/TIFF page-count percentiles;
- candidates already covered by embedded text;
- projected raster/temp/text/index storage;
- estimated duration by format family from measured samples.

Preflight output contains node counts and aggregates, not filenames, email headers, or extracted content by default. A separate explicitly requested bounded error sample may contain node IDs for troubleshooting.

## OCR epic amendments

OCR's completed implementation remains valid, but it does not yet coordinate a 700,000-file historical rollout. [Story 16.6](ocr.md) is added as the required production amendment.

| Existing OCR story       | Required backfill interpretation/change                                                                                                                |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 14.3 Resource limits     | Retain per-file limits; additionally require shared cross-process `heavy_cpu` admission and container CPU quotas through Story 16.6.                   |
| 14.5 Automatic enqueue   | Scope remains newly finalized files only. Never turn it into migration/startup enumeration of existing nodes.                                          |
| 14.6 Manual reprocessing | Keep for bounded repair and version mismatch. It is not the historical scheduler and must not be used repeatedly to manufacture an untracked campaign. |
| 16.1 Status API          | Extend/compose status with foreground-versus-campaign counts, campaign limits, throughput, and ETA.                                                    |
| 16.2 Event stream        | Attach optional campaign IDs and emit campaign state/refill/health events without breaking existing per-file events.                                   |
| 16.4 OCR page            | Add preflight and campaign start/pause/resume/cancel controls, with explicit confirmations and canary evidence.                                        |
| 16.6 Historical campaign | Implements the shared schema, scheduler integration, priority, resource lease, UI, tests, and deployment invariants in this document.                  |

The current OCR reprocess batch limit of 100 is retained as a repair bound. A campaign also uses batches of 100 initially, but unlike repeated reprocess calls it owns a durable cursor, snapshot, totals, pause state, queue ceiling, and audit trail.

## Email epic amendments

[Email Stories 17.1, 17.2, 17.3, 18.6, 19.1, 20.1, and 22.1–22.5](email.md) incorporate this plan directly:

- Shared backfill schema is migration 0017; email storage follows it.
- Automatic email extraction applies to newly finalized files only.
- Historical email starts as an explicitly confirmed paused campaign.
- Email search-vector population and index creation cannot block application startup with an archive-wide rewrite.
- Foreground email, repair, and historical jobs are distinguishable.
- Email, OCR, and attachment processing share resource admission.
- Status/UI exposes campaign controls and does not conflate paused history with stuck new mail.
- End-to-end validation follows the exact rollout stages below.

## Migration strategy

### Required application change

Previously both API and worker called the SQLx migrator during startup. Production-scale backfill migrations now use a one-shot production migration path:

- add a small `strife-migrate` binary or equivalent one-shot Compose service;
- add `RUN_MIGRATIONS=false` support for normal production API/worker startup;
- keep automatic migrations available in development/test if desired;
- serialize production migration execution explicitly;
- record migration version, start/end time, and result in deployment logs.

**Implementation report:** Phases 1 and 2 of the implementation order are complete. `strife-migrate` is a dedicated one-shot binary and production Compose prerequisite; API and worker startup migrations are disabled with `RUN_MIGRATIONS=false` while development retains automatic migration by default. Migration `0017_backfill_campaigns` adds inert draft campaigns, append-only events, job provenance, campaign linkage, resource classes, renewable database capacity slots, and foreground-fairness state. The API exposes campaign list/detail/create/prepare/action endpoints plus resumable SSE events. The worker has a `BACKFILL_ENABLED=false` coordinator and provider contract, but no OCR or email candidate provider is registered yet, so this foundation cannot enumerate or enqueue the historical library. PostgreSQL integration coverage applies the migration from scratch and verifies explicit prepare/resume, paused claim blocking, cancellation safety, shared-slot exclusion, and the fairness budget.

**Remaining before any historical run:** OCR preflight/candidate selection, transactional refill-and-cursor advancement, OCR campaign UI/status integration, multi-process restart/expiry tests, production-shaped migration benchmarks, and the Orion backup/canary procedure remain open. Turning `BACKFILL_ENABLED` on today still performs no historical work because the provider registry is empty.

### Migration rules

- Initial deploy migrations are additive and backward-compatible with the currently running image.
- No migration enumerates candidates, enqueues historical jobs, parses files, populates historical email/OCR text, or builds a large populated index synchronously.
- Prefer adding nullable columns, backfilling in bounded batches, validating, then adding stricter constraints.
- Build indexes on new empty tables before backfill where possible.
- If a large populated index must be added, use a separately controlled PostgreSQL-safe/concurrent operation rather than an API/worker startup transaction.
- Measure free disk before index creation; PostgreSQL may temporarily require substantial additional space.
- Production rollback normally leaves additive schema in place and rolls application images back. Do not run destructive down migrations against the live archive as an automatic rollback step.

## Exact Orion deployment sequence

This is the target workflow after the campaign/migration tooling exists. Commands are illustrative until a repository deployment script implements and verifies them.

### Phase 0 — Implementation gate

Before touching Orion:

- all OCR Story 16.6 and referenced email acceptance criteria pass;
- migration up/down behavior is tested on a production-shaped database copy;
- new and old application images are both compatible with the additive schema;
- the worker image contains required parser/OCR tools;
- Compose validates with campaigns disabled and explicit CPU/memory limits;
- backup and rollback procedures have been rehearsed.

### Phase 1 — Baseline and backup

1. Record the current revision, container images, job counts, active import state, database size, storage free space, API latency, and Orion resource baseline.
2. Stop initiating new manual import scans. Existing upload/download/API use may continue.
3. Allow an active import scan to reach a safe job boundary, or gracefully stop the worker and rely on durable lease recovery. Do not kill storage/database operations abruptly.
4. Take and verify a consistent PostgreSQL backup and the configured storage snapshot/backup containing canonical originals.
5. Run the read-only email and OCR preflight commands against the existing deployment or a tooling-only image. Confirm they created no jobs and changed no projection counts.

### Phase 2 — Build and stage images

1. Build immutable API, worker, web, and migration images tagged with the exact revision.
2. Verify manifests, architecture, extractor versions, and configuration offline.
3. Keep the previous image tags available locally for rollback.
4. Stage environment values with `BACKFILL_ENABLED=false`, campaign capacities of 1, and the conservative queue/batch defaults.

### Phase 3 — Additive migration

1. Keep the old API serving if the migration is confirmed backward-compatible.
2. Stop the worker or leave it drained so no old process races a newly introduced job type.
3. Run the one-shot migration service exactly once.
4. Verify the migration version, constraints, indexes on empty/new tables, job counts, and absence of historical campaign jobs.
5. On migration failure, stop and restore/repair according to the tested runbook; do not continue into application rollout.

### Phase 4 — Foreground-only application rollout

1. Start the new API with startup migrations disabled; verify readiness and existing file operations.
2. Start the new web image; verify All Files, Imports, OCR, Email, Errors, downloads, and previews.
3. Start `worker-interactive` with backfill origin disabled and global/per-resource concurrency at 1.
4. Verify a newly uploaded synthetic image receives foreground OCR and a newly uploaded synthetic `.eml` receives foreground email parsing.
5. Verify existing historical files produced zero new OCR/email jobs.
6. Verify import recovery and ordinary queues remain healthy before re-enabling manual import scans.
7. Observe the foreground-only deployment for an agreed soak period before creating a campaign.

This phase introduces only the normal brief restart/drain interruption for each single-instance service. Durable jobs recover after worker restart; API/web remain independently restartable. It does not start historical CPU work.

### Phase 5 — Email canaries

1. Create an email campaign from the reviewed preflight report in `paused` state.
2. Confirm candidate count, snapshot, parser version, batch/queue/running limits, resource class, disk projection, and backup status.
3. Start a 100-message canary; let it drain completely and pause.
4. Review correctness, failure/warning categories, throughput, p50/p95 time, CPU, memory, temperature/throttling, I/O wait, database/index growth, API latency, and search/render safety.
5. Repeat with 1,000 and 10,000 messages only after explicit approval at each gate.
6. Adjust provisional limits downward when uncertain; do not increase concurrency during the same canary used to establish a baseline.

### Phase 6 — Full email body campaign

1. Resume the email campaign with the proven batch/queue settings.
2. Keep `max_running=1` initially and retain the shared resource capacity of 1.
3. Monitor campaign status and host/application health; pause automatically on a health gate and manually on unexplained regressions.
4. Allow foreground work to preempt historical claims.
5. Reconcile candidate/completed/failed/skipped counts and run search correctness/index-health checks before marking complete.

### Phase 7 — Historical ordinary OCR

1. Create the OCR campaign only after email body extraction/indexing is complete or deliberately paused long-term.
2. Repeat 100-, 1,000-, and 10,000-file canaries, measuring each MIME family separately.
3. Start full OCR with one running job and one shared heavy-CPU slot.
4. Do not run email attachment OCR concurrently.
5. Reconcile text status, failures, search index growth, temp-space behavior, and resource measurements at completion.

### Phase 8 — Attachments

Run in this order, each as a separate canary-gated campaign:

1. attachment materialization/manifest repair;
2. non-OCR attachment text extraction;
3. attachment OCR.

Attachment OCR uses the same OCR and `heavy_cpu` permits as ordinary OCR. It never increases total Tesseract concurrency implicitly.

### Phase 9 — Closeout

- Record final counts, versions, failures, throughput, database/index/artifact growth, resource limits, and elapsed time.
- Update Orion performance documentation with measured settings.
- Export a safe campaign report and retain it with deployment records.
- Keep repair/reprocess controls bounded; completing a campaign does not authorize automatic future full-library version reprocessing.
- Run backup and restore validation after the new database/index footprint stabilizes.

## Backup and restore requirements

The email feature changes what a backup has to cover, because it creates a large
volume of data that is *not* worth backing up alongside a small volume that
absolutely is.

**Canonical and irreplaceable — must be in every backup and every restore drill:**

- The `.eml` originals in managed storage. Nothing can rebuild these. If they are
  lost, the archive is lost, and every projection below becomes an orphan
  describing messages that no longer exist.
- The `nodes` and `file_objects` rows that give those originals identity.

**Derived and rebuildable — restore is a convenience, not a requirement:**

- `email_messages`, `email_addresses`, `email_headers`, `email_labels`,
  `email_attachments` — reparsed from the originals.
- `email_attachment_artifacts` and their storage objects — rematerialized.
- `email_attachment_text` — re-extracted.
- `search_vector` — rebuilt by the bounded index backfill.

Rebuilding all of it means a full campaign, which is hours to days of sustained
CPU, so restoring the projections is worth doing when they are available. Losing
them is a delay; losing an original is permanent.

Requirements before the full backfill starts:

1. A verified PostgreSQL backup taken **after** the additive migration and
   **before** the first campaign, so a rollback target exists that already has
   the new schema.
2. A storage backup or snapshot covering the originals namespace. Artifact and
   staging namespaces may be excluded to keep the backup small — both are
   regenerable, and the artifact namespace grows by roughly the archive size
   again once attachments are materialized.
3. A restore drill that proves the originals come back readable: restore to a
   scratch host, run the read-only preflight against the restored storage, and
   confirm the file count and total bytes match the pre-backup survey.
4. Backup free space checked against the projected database growth from the
   preflight report. A backup that silently stops mid-campaign is worse than one
   that was never configured.

## Canary promotion gates

A canary advances only when:

- no data-integrity or original-file mutation issue exists;
- no OOM, container restart, thermal throttle, disk exhaustion, or stuck lease occurred;
- API readiness and interactive latency remain within the preflight baseline's approved tolerance;
- database locks, connections, vacuum, and free disk remain healthy;
- failure/warning categories are understood and the projected full-run failure volume is acceptable;
- extracted/search/rendered samples are semantically and security reviewed;
- measured throughput produces a credible completion estimate;
- pause, resume, and restart recovery have been exercised successfully.

Promotion is an explicit operator action. Time passing without an alarm is not approval.

## Pause, cancellation, and rollback

### Pause

Pause the campaign first. The scheduler stops refilling, workers stop claiming its pending jobs, and leased work drains. Foreground services remain running.

### Cancel

Cancellation transitions pending campaign jobs to `skipped` with an audited reason and drains leased work. Completed OCR/email projections remain available and regenerable. Original files are untouched.

### Application rollback

1. Pause every campaign and drain leased work.
2. Disable `worker-backfill` and keep `BACKFILL_ENABLED=false`.
3. Roll API/web/worker images back to the previously verified tag in compatibility order.
4. Leave additive tables/columns in place unless a separately reviewed destructive rollback is required.
5. Verify old workers cannot claim unknown new job types and foreground legacy operations remain healthy.
6. Preserve campaign and event rows for diagnosis.

A rollback does not attempt to remove already generated email/OCR projections. They are linked by cascade, versioned, and may be ignored or replaced by a later forward fix.

## Observability

Campaign status and dashboards should expose:

- state, kind, snapshot, cursor, limits, and versions;
- candidate, enqueued, pending, running, completed, failed, skipped, and remaining counts;
- foreground versus backfill queue depth;
- current resource leases and expiry;
- files/minute and bytes/minute;
- p50/p95/p99 duration by job and MIME family;
- ETA with the assumptions used;
- worker/container CPU and memory;
- host load, thermal/throttling, I/O wait, and free disk;
- PostgreSQL size/index growth, connections, lock waits, vacuum progress, and slow queries;
- recent safe warnings and campaign state transitions.

SSE updates the UI; page refresh is neither required nor used for progress.

## Test requirements

The shared implementation is incomplete until tests prove:

- deploying/migrating/starting with 700,000 synthetic candidate rows creates no historical jobs;
- foreground new-file processing works while every campaign is paused;
- queue depth never exceeds campaign limits under concurrent scheduler ticks;
- cursor/refill is idempotent across coordinator restart and transaction failure;
- active jobs are excluded before batch limits;
- foreground priority and fairness work under sustained mixed load;
- one shared heavy resource slot prevents OCR/email/attachment overlap across multiple worker processes;
- resource leases renew, expire, and recover with job leases;
- pause, drain, resume, cancel, and health-pause state transitions are durable;
- migration execution is serialized and ordinary startup does not run heavy migrations;
- image rollback with additive schema preserves foreground functionality;
- SSE reconnects without replaying full history or leaking database connections;
- canary and full-run reconciliation detect missing, duplicate, or orphan projections.

## Initial recommendation for Orion

Start conservatively:

```text
WORKER_CONCURRENCY=1
EXTRACTOR_CONCURRENCY=1
PREVIEW_CONCURRENCY=1
OCR_CONCURRENCY=1
EMAIL_PARSE_CONCURRENCY=1
ATTACHMENT_EXTRACTION_CONCURRENCY=1
HEAVY_CPU_CONCURRENCY=1
BACKFILL_BATCH_SIZE=100
BACKFILL_MAX_QUEUED=500
BACKFILL_MAX_RUNNING=1
FOREGROUND_CLAIMS_PER_BACKFILL=20
BACKFILL_ENABLED=false
```

The first production deployment keeps `BACKFILL_ENABLED=false`. Enabling the scheduler later still leaves every campaign paused until the operator starts one. Concurrency increases require canary evidence; they are not a reward for a campaign merely appearing stable.
