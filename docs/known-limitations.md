# Known Limitations (v1)

User-facing summary of what Strife v1 does **not** include. Deferred work lives in [`deferred.md`](../deferred.md).

## Accounts and network

- No sign-in, passwords, or multi-user accounts
- Designed for a **private LAN** only; not hardened for the public internet
- No share links

## Files and storage

- No file version history
- No content deduplication (checksums are for integrity only)
- Duplicate sibling names are rejected (no auto-rename)
- No symbolic links or special-file imports
- Single fixed import inbox (`/mnt/ext/watch` → root); manual scan only
- Trash retention is 30 days; permanent delete is irreversible

## Media

- No video transcoding for unsupported codecs
- OCR is print-text only; handwriting recognition and user corrections are not supported
- `OCR_MEMORY_LIMIT_BYTES` constrains ImageMagick normalization, while Tesseract relies on the worker container's 1.5 GiB ceiling rather than a per-process kernel memory limit
- No global filename/metadata search
- No gallery/grid or mobile layout (desktop table only)

## Operations

- Production Compose stack is **LAN self-host only** (no auth/TLS productization; operator-supplied reverse proxy if needed)
- No automated backup/restore product
- Worker concurrency must be kept low on 4 GB hosts (see [performance.md](performance.md))
- OCR page, pixel, time, memory, and text defaults are provisional until Orion profiling is complete
- Image tags/revisions and host paths (`/srv/strife/*`) must be set by the operator

## Interface

- Command bar is filesystem-like, not a full shell
- Multi-file download as ZIP is deferred
- Themes: true-black dark + light only
