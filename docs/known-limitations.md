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
- Single configurable import inbox (`IMPORT_WATCH_ROOT` → root); manual scan only
- Trash retention is 30 days; permanent delete is irreversible

## Media

- No video transcoding for unsupported codecs
- OCR is print-text only; handwriting recognition and user corrections are not supported
- `OCR_MEMORY_LIMIT_BYTES` constrains ImageMagick normalization, while Tesseract relies on the worker container's 1.5 GiB ceiling rather than a per-process kernel memory limit
- No global filename/metadata search
- No gallery/grid or mobile layout (desktop table only)

## Email archive

- Email parsing runs **in-process in the worker**, not in a separate process or sandbox. Limits on source bytes, MIME parts, header size, body size, attachment size, and wall time bound what one message can consume, and the container's CPU and memory ceilings bound the worker as a whole — but a memory-safety bug in the MIME parser would run with the worker's privileges rather than inside a jail. This matches how Tika and Tesseract are run today (external processes for those, in-process for MIME), and closing it means moving parsing to a child process with its own rlimits.
- Attachment transfer decoding is **not streamed**: `mail-parser` builds the whole message in memory before any part is addressable, so peak memory scales with message size up to `EMAIL_MAX_SOURCE_BYTES`. Hashing and writing are streamed. True streaming decode requires a different parser or a hand-written MIME walk.
- Email parser and attachment limits are provisional until Orion profiling is complete, in the same way the OCR defaults are.
- Gmail labels and thread IDs are preserved as **imported facts**. Strife never re-contacts Gmail, so they reflect the export and can drift from the live mailbox.
- Thread grouping is inference from headers, not authoritative. A message with no `Message-ID` or `References` falls back to normalized subject, which can join unrelated mail; the basis is recorded per message and shown in the reader.

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
