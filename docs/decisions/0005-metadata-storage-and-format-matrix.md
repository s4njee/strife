# ADR 0005: Preserve Raw Metadata and Normalize Common Fields

- **Status:** Accepted
- **Date:** 2026-07-29

## Context

Strife needs rich, reprocessable metadata without forcing every extractor-specific field into the relational schema. Local measurements found ExifTool JSON commonly below 7 KB, ffprobe JSON commonly between 3 KB and 22 KB with an observed 78 KB outlier, and Tika metadata-only JSON commonly below 15 KB. The capacity target is approximately 10–15 GB of raw JSON per one million files.

## Decision

Every successful extractor result is stored in full as PostgreSQL `jsonb`; v1 does not truncate it, select only a subset, expire it, or move it to sidecars. The 10–15 KB-per-file figure is a capacity-planning average, not an individual record limit. A bounded extractor subprocess/response reader may reject pathological output as a failed extraction, but it never stores partial JSON.

Tika metadata JSON is distinct from extracted document text. Document text and OCR text use separate records and capacity policies; they are not embedded into metadata payloads.

Frequently displayed, sorted, or filtered values live in a one-to-one `node_metadata` record: detected MIME, media kind, duration, width, height, capture time, page count, orientation, GPS presence and coordinates, camera make/model, document title/author and document creation/modification dates. Per-stream codec, bitrate, dimensions, frame rate, and language live in `media_streams`. Checksums remain on `file_objects`.

## v1 Acceptance Matrix

| Kind | Formats | Required extractor coverage |
|---|---|---|
| Documents | DOC, DOCX, PDF | content MIME + Tika metadata |
| Images | JPEG, GIF, PNG | content MIME + ExifTool |
| Raw photos | NEF, DNG | content MIME + ExifTool |
| Video | MP4, MKV, MOV | content MIME + ffprobe |
| Audio | MP3, M4A | content MIME + ffprobe |
| Other | any regular file | content MIME + generic unsupported record |

Fixtures may be synthetic when licensing prevents committing a real sample, but every listed row must run in the acceptance suite. Extractor versions are recorded with every result.

## Consequences

- Raw data can be reinterpreted after schema changes without rerunning tools immediately.
- PostgreSQL metadata capacity is planned at 10–15 GB per million files and monitored operationally.
- Successful unusually large payloads remain intact; malicious or runaway tool output fails the job at the process safety boundary.
- Typed columns remain intentionally compact while uncommon fields stay queryable in JSON.
