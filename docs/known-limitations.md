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
- No OCR or document text search
- No global filename/metadata search
- No gallery/grid or mobile layout (desktop table only)

## Operations

- Development-oriented Docker Compose; production packaging deferred
- No automated backup/restore product
- Worker concurrency must be kept low on 4 GB hosts (see [performance.md](performance.md))

## Interface

- Command bar is filesystem-like, not a full shell
- Multi-file download as ZIP is deferred
- Themes: true-black dark + light only
