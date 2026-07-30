# ARM64 / Raspberry Pi 5 Validation

Strife's primary host is **Gentoo Linux on Raspberry Pi 5 (ARM64, 4 GB RAM)**. This document records how to validate that target and the recommended concurrency for the 4 GB envelope.

## Prerequisites on the Pi

- Rust stable with `rustfmt` and `clippy`
- Node.js 22+
- Docker (or a native PostgreSQL 16+ and Apache Tika)
- Host tools: `file`/libmagic, ExifTool, ffmpeg/ffprobe, ImageMagick `convert`, LibreOffice (`soffice`) for DOCX previews when testing those paths

Confirm extractors:

```sh
uname -m   # expect aarch64
file --version
exiftool -ver
ffmpeg -version
ffprobe -version
convert -version
```

## Validation procedure

From the repository root on the Pi:

```sh
cp -n .env.example .env
# Point DATABASE_URL at local Compose Postgres if needed
docker compose -f docker-compose.dev.yml up -d --wait

export DATABASE_URL="${DATABASE_URL:-postgresql://strife:strife-dev@127.0.0.1:5432/strife}"
export SQLX_OFFLINE=true

./scripts/validate-arm64.sh
```

The script runs format/clippy/build/test (Rust workspace + frontend) and records basic process memory samples. Monitor for OOM:

```sh
dmesg -T | grep -i 'out of memory\|killed process' || true
journalctl -k -b | grep -i 'out of memory\|oom' || true
```

## Recommended 4 GB settings

| Variable | Recommended | Notes |
|---|---|---|
| `WORKER_CONCURRENCY` | `1`–`2` | Default `2`; drop to `1` if ExifTool/ffmpeg pressure is high |
| `EXTRACTOR_CONCURRENCY` | `1` | Serialize heavy extractors |
| `PREVIEW_CONCURRENCY` | `1`–`2` | Thumbnail/preview generation |
| `WORKER_POLL_INTERVAL_SECONDS` | `5` | Default is fine |
| `WORKER_LEASE_TTL_SECONDS` | `300` | Matches long LibreOffice conversions |

PostgreSQL (Compose defaults are adequate for single-user LAN; see [performance.md](../performance.md) for shared_buffers guidance).

## Container images

Development images in `docker-compose.dev.yml` are multi-arch (`linux/arm64` and `linux/amd64`) digest-pinned. On the Pi:

```sh
docker compose -f docker-compose.dev.yml pull
docker compose -f docker-compose.dev.yml up -d
docker image inspect postgres:17.10-alpine3.24 --format '{{.Architecture}}'
```

## Validation status

| Check | Status | Notes |
|---|---|---|
| Host tools available on ARM64 developer machine | Verified | `file`, ExifTool, ffmpeg present on `aarch64-apple-darwin` |
| Workspace unit/integration tests with PostgreSQL | Verified | Lifecycle, import, edge-case suites pass when `DATABASE_URL` is set |
| OOM under full suite | No kills observed | Re-check on Pi under concurrent upload + metadata load |
| Pi 5 native full suite | Operator step | Run `./scripts/validate-arm64.sh` on the device and attach results |

Keep this file updated when Pi measurements change.
