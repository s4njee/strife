#!/usr/bin/env bash
# Validate Strife on ARM64 (Raspberry Pi 5 or other aarch64 hosts).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> Architecture"
uname -a
uname -m

echo "==> Extractor tools"
for cmd in file exiftool ffmpeg ffprobe; do
  if command -v "$cmd" >/dev/null 2>&1; then
    echo "  ok  $cmd ($("$cmd" -version 2>&1 | head -1 || "$cmd" -ver 2>&1 | head -1))"
  else
    echo "  WARN missing $cmd"
  fi
done

echo "==> Memory snapshot (before)"
if command -v free >/dev/null 2>&1; then
  free -h
elif command -v vm_stat >/dev/null 2>&1; then
  vm_stat | head -10
fi

export SQLX_OFFLINE="${SQLX_OFFLINE:-true}"
if [[ -z "${DATABASE_URL:-}" ]]; then
  echo "WARN: DATABASE_URL unset; PostgreSQL integration tests will skip"
fi

echo "==> Rust fmt / clippy / build / test"
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace

echo "==> Frontend"
npm --prefix apps/web ci
npm --prefix apps/web run lint
npm --prefix apps/web run build

echo "==> Memory snapshot (after)"
if command -v free >/dev/null 2>&1; then
  free -h
elif command -v vm_stat >/dev/null 2>&1; then
  vm_stat | head -10
fi

echo "==> OOM kernel messages (best effort)"
if command -v dmesg >/dev/null 2>&1; then
  dmesg 2>/dev/null | grep -i 'out of memory\|killed process' | tail -20 || echo "  none found"
fi

echo "ARM64 validation finished successfully."
