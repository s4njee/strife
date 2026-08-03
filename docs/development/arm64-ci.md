# ARM64 continuous integration

GitHub Actions runs the complete formatting, zero-warning Clippy, build, test,
frontend, and deployment-image gates natively on `ubuntu-24.04-arm`. The same
gates run on x86-64. The image job builds the migration, API, worker, and web
targets and verifies Tesseract plus the English language data inside the worker.

The ARM64 runner is currently a GitHub public-preview image. A normal warm-cache
check is expected to take roughly 15–30 minutes; container builds may take
longer when the Rust or Debian layers are cold. The Actions job summary is the
authoritative runtime record. If preview-runner availability becomes unstable,
move the ARM64 jobs to `push` on `main` rather than silently removing them.

`scripts/validate-arm64.sh` intentionally keeps the checks CI cannot reproduce:
the actual Orion kernel/architecture, installed extractor visibility, before
and after memory snapshots, and kernel OOM messages.
