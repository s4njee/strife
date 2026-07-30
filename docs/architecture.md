# Architecture

## Overview

```text
SolidJS web app
      |
      | LAN HTTP / JSON / byte ranges
      v
Axum API -------------------------------- PostgreSQL
      |                                       | nodes, file_objects
      |                                       | upload_sessions, jobs
      |                                       | metadata, artifacts
      v                                       | import_sources/entries
Storage abstraction                           | trash_entries, favorites
      |
      v
Local filesystem (ZFS-backed in production)
  staging/  originals/  artifacts/

Background worker <--- jobs table (SKIP LOCKED)
  ExifTool, ffprobe, Tika, ffmpeg, LibreOffice, LibRaw

Watched folder (/mnt/ext/watch) --> importer --> same finalize pipeline
```

## Crates

| Crate | Role |
|---|---|
| `strife-api` | Axum HTTP API, config, health, uploads, folders, nodes, files, imports |
| `strife-worker` | Job loop: metadata, preview, permanent deletion, trash cleanup enqueue |
| `strife-db` | SQLx migrations and typed queries |
| `strife-domain` | Pure folder/name/move rules |
| `strife-storage` | Opaque UUID keys, staging/originals/artifacts, disk guard |
| `strife-media` | MIME, ExifTool, ffprobe, Tika, thumbnails, office conversion |
| `strife-importer` | Manual scan, stability, import finalization, recovery |

Frontend: `apps/web` — SolidJS + TypeScript + Vite.

## Data flow highlights

1. **Upload:** `POST /api/uploads` → staged object + session → `PATCH` chunks with `Content-Range` → `POST .../finalize` creates node/file_object and enqueues metadata.
2. **Import:** User triggers scan → discover inbox files → stage with size/mtime stability → finalize like uploads → remove source.
3. **Metadata/preview:** Worker claims jobs; extractors write `metadata_records` / `node_metadata` / `derived_artifacts`.
4. **Trash:** Soft-delete to `trashed` + `trash_entries`; restore reactivates; permanent deletion job removes storage then rows.
5. **Disk guard:** At ≥ 90% projected usage, new uploads/imports return 507.

## Key decisions

| ADR | Topic |
|---|---|
| 0001 | Primary host: Gentoo ARM64 Pi 5 |
| 0002 | Direct ZFS storage (no MinIO in v1) |
| 0003 | Managed layout under `/mnt/ext/strife` |
| 0004 | Fixed watch path, manual scan, move-after-import |
| 0005 | Metadata retention and format matrix |
| 0006 | LibreOffice + LibRaw preview tools |
| 0007 | Command-bar grammar and commands |

## Security posture (v1)

No authentication. Bind only on a trusted LAN. Do not expose the API to the public internet without a reverse proxy and future auth (v2).
