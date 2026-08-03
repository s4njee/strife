# ADR 0008: Persist Explicit OCR and Document Text in PostgreSQL

- **Status:** Accepted
- **Date:** 2026-08-02

## Context

Strife needs document-content search across the whole drive. Apache Tika can
extract an embedded PDF text layer, and its full container may also invoke its
bundled Tesseract parser while handling scanned PDFs. Strife currently requests
metadata only, so text recognized implicitly by Tika is discarded after
consuming CPU. A dedicated text contract is required before OCR, search, and UI
work can share durable results without running OCR twice.

## Decision

- English (`eng`) is the initial OCR language. Language selection is one global
  worker setting rather than automatic or configurable per file.
- PDF, JPEG, PNG, TIFF, WebP, and raw camera images are OCR candidates. All
  supported inputs are considered automatically after finalization.
- PDFs are checked for usable embedded text before OCR. Any meaningful text
  layer is stored as `embedded` text and causes explicit OCR to be skipped. The
  minimum non-whitespace character threshold is configurable so an empty layer
  or stray glyph does not suppress OCR.
- Document text and OCR text use one PostgreSQL representation with document
  and page records. It retains page boundaries, language, engine name and
  version, page-level confidence, dimensions, warnings, duration, and aggregate
  character count and confidence.
- OCR confidence is the character-count-weighted mean of non-negative
  Tesseract word confidences. Words without a usable confidence are excluded.
- Text is read-only and regenerable. The interface may display and copy it but
  does not offer corrections. Handwriting recognition is not planned.
- Strife does not generate searchable-PDF derivatives with OCRmyPDF. The goal
  is cross-drive PostgreSQL search and in-app text access, not a second
  downloadable copy of each original.
- Metadata-only Tika PDF requests must explicitly select Tika's `NO_OCR`
  strategy. The dedicated OCR job is the only path allowed to invoke
  Tesseract, preventing scanned PDFs from being OCR'd twice.
- OCR is manually re-triggerable for one file or a bounded set. Engine or
  language version changes do not automatically enqueue the entire library.
- Page, pixel, time, memory, and stored-output limits begin with documented
  conservative defaults. Those values are provisional until Orion profiling
  supplies peak-memory and duration measurements.

## Alternatives Considered

- Retain the text Tika happens to produce during metadata extraction
- Run Tika's implicit OCR and a separate Tesseract job
- Generate and store searchable PDF derivatives with OCRmyPDF
- Store OCR output as sidecar files in managed storage
- Allow per-file language selection or automatic language detection initially
- Permit users to edit recognized text

## Consequences

- OCR, embedded PDF text, search, status reporting, and the text UI share one
  durable schema and provenance model.
- Metadata extraction must disable Tika OCR before the explicit OCR worker is
  enabled in production.
- PostgreSQL capacity planning must include document text and its full-text
  index, while object storage does not need duplicate searchable PDFs.
- Language expansion requires installing another Tesseract language pack,
  changing the global setting and PostgreSQL's `english` full-text
  configuration, rebuilding generated search vectors, and manually scheduling
  bounded reprocessing.
- Initial resource-limit values remain an operational hypothesis and must be
  revisited with measurements rather than treated as permanent policy.
