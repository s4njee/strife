# Performance Tuning (4 GB Raspberry Pi 5)

Single-user LAN deployment with limited RAM. Prefer serializing extractors over maximizing throughput.

## Process concurrency

| Setting | Env var | Default | 4 GB recommendation |
|---|---|---|---|
| Worker job processors | `WORKER_CONCURRENCY` | 2 | **1–2** |
| External extractors (ExifTool / ffprobe / Tika slots) | `EXTRACTOR_CONCURRENCY` | 1 | **1** |
| Preview generators | `PREVIEW_CONCURRENCY` | 2 | **1–2** |
| Worker poll idle | `WORKER_POLL_INTERVAL_SECONDS` | 5 | 5 |
| Job lease TTL | `WORKER_LEASE_TTL_SECONDS` | 300 | 300–600 for large DOCX |

LibreOffice conversions are serialized in-process. Prefer `PREVIEW_CONCURRENCY=1` if DOCX preview and thumbnails compete.

## PostgreSQL (development Compose)

For a 4 GB host shared with API/worker/Tika, keep Postgres modest:

| Setting | Suggested | Rationale |
|---|---|---|
| `shared_buffers` | 256 MB | ~6% of RAM |
| `work_mem` | 8–16 MB | Avoid multi-hash explosion |
| `maintenance_work_mem` | 64 MB | Index/vacuum |
| `max_connections` | 30 | API + worker + admin |

Example `postgresql.conf` fragment (production packaging is deferred):

```conf
shared_buffers = 256MB
work_mem = 16MB
maintenance_work_mem = 64MB
effective_cache_size = 1GB
max_connections = 30
```

Compose defaults are acceptable for light development.

## Expected extractor timings (order-of-magnitude)

Measured on Apple Silicon ARM64 developer host with representative fixtures; re-measure on the Pi for absolute numbers.

| Operation | Typical | Notes |
|---|---|---|
| libmagic MIME | &lt; 50 ms | `file --mime-type` |
| ExifTool JPEG/PNG | 100–400 ms | Full JSON payload |
| ExifTool NEF/DNG | 0.5–3 s | Depends on embedded preview |
| ffprobe short MP4 | 50–200 ms | |
| Tika small PDF/DOCX | 0.5–2 s | Network to local Tika |
| Image thumbnail 256px | 100–500 ms | ffmpeg/ImageMagick |
| DOCX → PDF (LibreOffice) | 1–5 s cold | Serialized; high RSS |
| RAW half-decode → WebP | 1–4 s | When no embedded JPEG |

## Memory envelope (planning)

Rough resident set under light concurrent use (order-of-magnitude):

| Component | Typical RSS |
|---|---|
| `strife-api` | 30–80 MB |
| `strife-worker` (idle) | 20–50 MB |
| Worker + one ExifTool/ffmpeg | +50–300 MB peak |
| LibreOffice conversion | +200–400 MB peak |
| PostgreSQL | 50–150 MB |
| Apache Tika | 200–500 MB |
| **Sum under load** | aim &lt; 3 GB leaving headroom |

If the Pi OOMs, reduce `WORKER_CONCURRENCY` and `PREVIEW_CONCURRENCY` to 1 and avoid simultaneous DOCX conversion and RAW decode.

## Jobs table steady state

The jobs table is append-only in practice: every upload, watched-folder import,
metadata extraction, preview, OCR, email parse, and attachment extraction adds a
row, and a backfill campaign adds one per candidate. Without a purge it grows for
the life of the deployment, and the partial indexes that make claiming fast are
built over an ever-larger heap.

Finished jobs are purged hourly in bounded batches. Successes are dropped after
`JOB_RETENTION_COMPLETED_DAYS` (7); failures and cancellations are kept for
`JOB_RETENTION_FAILED_DAYS` (30), because `last_error` and the attempt count are
what triage reads. Pending and leased jobs are never purged at any age, and a
failed job behind an unresolved entry in the Actionable Errors tab is retained
until that entry is resolved.

### A 100,000-file library

Steady state is governed by *throughput*, not library size — the table holds a
retention window's worth of work, not one row per file. For a library of 100,000
files with each file producing roughly three jobs over its life (metadata,
preview, and one extractor):

| Scenario | Rows in the window | Table + index size |
| --- | --- | --- |
| Idle library, no ingestion | ~0 | &lt; 1 MB |
| Steady use, ~200 new files/day | ~4,200 | ~3 MB |
| Heavy week, ~5,000 files/day | ~105,000 | ~60 MB |
| Mid-backfill, 100k queued + processed | ~300,000 | ~180 MB |

Rows average roughly 400 bytes with indexes. The backfill row is the one that
matters: a 691,000-message email campaign would leave about 4.8 million completed
rows and around 2 GB if nothing purged them, and would still be there months
later. With the 7-day window it drains to near zero within a week of the campaign
finishing.

If the table is ever found large after a long-neglected deployment, the purge
takes one `JOB_PURGE_BATCH` bite per hour by design rather than locking the table
for a single large delete. Raise `JOB_PURGE_BATCH` temporarily to drain faster,
and lower it again — a large batch during a backfill competes with job claiming
for the same rows.

## Load tips

- Stream uploads; never buffer whole files in the API.
- Prefer native browser preview for JPEG/PNG/GIF/WebP/PDF/audio/video to skip generation.
- Trash cleanup and permanent deletion are batch-limited (50/hour enqueue) to protect the worker.
- Finished jobs are purged hourly (500/batch); unfinished work is never purged.
- Disk guard at 90% rejects new ingestion before the volume fills.
