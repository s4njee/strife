# Setup Guide

Step-by-step development setup for Strife on macOS, Linux x86-64, and Raspberry Pi (ARM64).

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.85+ stable | `rustfmt`, `clippy` |
| Node.js | 22+ | Frontend toolchain |
| Docker | recent | PostgreSQL + Tika via Compose |
| `file` / libmagic | host | MIME detection |
| ExifTool | recent | Image/raw metadata |
| ffmpeg / ffprobe | recent | Media metadata and thumbnails |
| ImageMagick | 7+ | Some preview encodes |
| LibreOffice | optional | DOCX → PDF previews |

Optional for RAW: LibRaw `dcraw_emu` / `dcraw_half`.

## 1. Clone and environment

```sh
git clone <repo-url> strife
cd strife
cp .env.example .env
```

Edit `.env` if ports differ. Default database URL:

```text
DATABASE_URL=postgresql://strife:strife-dev@127.0.0.1:5432/strife
```

On some developer machines Compose maps Postgres to another host port (for example `55432`); set `POSTGRES_PORT` and `DATABASE_URL` accordingly.

## 2. Start dependencies

```sh
docker compose -f docker-compose.dev.yml up -d --wait
# or
make dev-services
```

Verify:

```sh
# Postgres
psql "$DATABASE_URL" -c 'SELECT 1'
# Tika
curl -sf "http://127.0.0.1:9998/tika" -H "Accept: text/plain" -d "hello" || true
```

## 3. Rust services

```sh
export $(grep -v '^#' .env | xargs)   # or source manually
mkdir -p .data/storage
export STORAGE_ROOT="${STORAGE_ROOT:-./.data/storage}"
export LISTEN_ADDR="${LISTEN_ADDR:-127.0.0.1:3000}"
export TIKA_URL="${TIKA_URL:-http://127.0.0.1:9998}"

cargo run -p strife-api
# separate terminal
cargo run -p strife-worker
```

Migrations apply automatically on API startup.

## 4. Frontend

```sh
npm --prefix apps/web ci
npm --prefix apps/web run dev
```

Open the Vite URL (default `http://127.0.0.1:5173`). The dev server proxies `/api/*` to the API.

## 5. Tests

```sh
export DATABASE_URL=postgresql://strife:strife-dev@127.0.0.1:5432/strife
export SQLX_OFFLINE=true
cargo test --workspace
npm --prefix apps/web run lint
npm --prefix apps/web run build
# or
make check
```

Integration tests skip cleanly when `DATABASE_URL` is unset.

## Platform notes

### macOS

- Install tools via Homebrew: `rustup`, `node`, `docker`, `exiftool`, `ffmpeg`, `imagemagick`, `libreoffice` as needed.
- Apple Silicon is `arm64`; use native builds. Cross-compile to Linux with `cross` or GNU linkers ([cross-compilation.md](development/cross-compilation.md)).

### Linux x86-64

- Native `cargo build --release --target x86_64-unknown-linux-gnu`.
- CI validates this target on every push to `main`.

### Raspberry Pi 5 (ARM64, 4 GB)

- Prefer native Gentoo/ARM64 toolchain.
- Set `WORKER_CONCURRENCY=1` or `2` and `EXTRACTOR_CONCURRENCY=1` ([performance.md](performance.md)).
- Run [validation/arm64.md](validation/arm64.md) / `./scripts/validate-arm64.sh`.

## 6. Production LAN self-host (Compose)

v1 ships a production Docker Compose stack for a single trusted LAN host:

```sh
# Required secret (do not use the dev password on a real host)
export POSTGRES_PASSWORD='choose-a-strong-password'
export STRIFE_IMAGE_TAG=latest          # or a pinned tag
export STRIFE_REVISION="$(git rev-parse --short HEAD)"

# Host data paths used by docker-compose.prod.yml
sudo mkdir -p /srv/strife/{postgres,storage,import}

docker compose -f docker-compose.prod.yml up -d --build
curl -sf http://127.0.0.1/api/ready
```

Required production variables are documented in `.env.example` (`POSTGRES_PASSWORD`, optional `STRIFE_IMAGE_TAG` / `STRIFE_REVISION`). The stack is intentionally **LAN-only**: no product auth or TLS termination. For a hardened host layout (systemd unit, ZFS paths), see [deploy/orion/README.md](../deploy/orion/README.md).

## Project layout

See [architecture.md](architecture.md).
