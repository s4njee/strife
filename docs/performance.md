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

## Load tips

- Stream uploads; never buffer whole files in the API.
- Prefer native browser preview for JPEG/PNG/GIF/WebP/PDF/audio/video to skip generation.
- Trash cleanup and permanent deletion are batch-limited (50/hour enqueue) to protect the worker.
- Disk guard at 90% rejects new ingestion before the volume fills.
