# Apache Tika extraction path

This document describes Strife's ingestion-to-Tika path and its boundary with
the explicit document-text pipeline defined by
[ADR 0008](decisions/0008-ocr-and-document-text.md). Tika remains the document
metadata extractor. Embedded and OCR text use separate PostgreSQL records; they
are not metadata JSON or an accidental side effect of Tika parsing.

## End-to-end flow

```text
upload or watched-folder import
          |
          v
finalize node + original file object
          |
          | atomically enqueue metadata_extraction + ocr jobs
          v
background worker copies original to a temporary file
          |
          v
libmagic content-MIME detection
          |
          +-- image/* --------------------------> ExifTool
          +-- video/* or audio/* ---------------> ffprobe
          +-- PDF, DOC, or DOCX ----------------> Apache Tika /meta
          +-- everything else ------------------> generic metadata record

parallel OCR job
          |
          +-- PDF --> Tika /tika with NO_OCR
          |              +-- usable text --> persist + skip OCR
          |              +-- no text -----> Poppler raster pages
          +-- JPEG/PNG/WebP/TIFF ----------> ImageMagick pages
          +-- RAW -------------------------> LibRaw preview --> ImageMagick
          +-- normalized pages ------------> Tesseract TSV --> PostgreSQL
```

Both `finalize_import` and `finalize_upload` enqueue durable
`metadata_extraction` and `ocr` jobs while publishing the file. OCR enqueue is
isolated behind a savepoint so an exceptional queue failure cannot hide an
otherwise finalized original; bounded manual reprocessing repairs that case. See
[`crates/db/src/lib.rs`](../crates/db/src/lib.rs).

The worker claims the job, copies the managed original to a temporary path, and
detects MIME from the file bytes rather than trusting its extension. Only these
document MIME types are routed to Tika:

- `application/pdf`
- `application/msword`
- `application/vnd.openxmlformats-officedocument.wordprocessingml.document`

Images are routed to ExifTool, so ordinary JPEG, PNG, GIF, and WebP files do not
enter the Tika/Tesseract path. See
[`crates/worker/src/metadata.rs`](../crates/worker/src/metadata.rs).

## Tika request

The adapter streams the complete temporary document to:

```http
PUT /meta
Accept: application/json
Content-Type: application/octet-stream
```

The current safety limits are:

- 60-second request timeout
- 16 MiB maximum JSON response
- complete-response semantics: an oversized, malformed, timed-out, or non-2xx
  response fails instead of storing a truncated result

The implementation is in
[`crates/media/src/tika.rs`](../crates/media/src/tika.rs).

Production uses `apache/tika:3.2.3.0-full` with a 1 GiB container memory limit
and a JVM heap range of `-Xms128m -Xmx768m`. Tika extraction is gated by
`EXTRACTOR_CONCURRENCY`; Orion currently sets it to `1`, so Strife submits at
most one Tika request at a time. See
[`docker-compose.prod.yml`](../docker-compose.prod.yml) and
[`performance.md`](performance.md).

## Tesseract and implicit OCR

The full Tika image includes Tesseract, but Strife's metadata and embedded-text
requests explicitly send `X-Tika-PDFOcrStrategy: no_ocr`. Tika therefore never
invokes its bundled OCR parser for Strife requests. The dedicated OCR job is
the only path allowed to run Tesseract, preventing scanned PDFs from being
recognized twice.

`/meta` output remains metadata-only. A separate `/tika` request checks the
embedded PDF text layer with the same `no_ocr` strategy. Usable text is stored
directly; otherwise Poppler rasterizes the PDF and the explicit Tesseract
adapter returns TSV text and confidence.

## Persisted results

The complete JSON returned by `/meta` is stored in `metadata_records.raw_payload`
under extractor name `tika` and version `adapter-v1`. Extractor warnings are
stored alongside it.

The adapter reads these common properties from the response:

- title
- author
- creation date
- modification date
- page count
- word count

The worker currently normalizes only the following into `node_metadata`:

- `page_count`
- `document_title`
- `document_author`

Creation date, modification date, and word count remain available only when
present in the stored raw Tika JSON. The schema is defined in
[`crates/db/migrations/0007_metadata.up.sql`](../crates/db/migrations/0007_metadata.up.sql).

## Failure and retry behavior

If Tika extraction fails, the worker stores a failed `tika` metadata record
containing the error and marks the durable job attempt failed. The generic job
queue retries until its configured maximum attempt count is reached. The
temporary document copy is removed after each attempt whether extraction
succeeds or fails.

Tika metadata extraction does not block the original upload or import from
being finalized. The original remains accessible even when metadata extraction
ultimately fails.

## OCR boundary and rollout

The explicit OCR rollout adds retained embedded text, retained OCR text, image
OCR, a global language setting, resource accounting, scheduling, and document
search in dependency order. Until each stage lands, its absence must not be
inferred from Tesseract being present in the Tika container.

Handwriting recognition and user-edited OCR text are deliberately excluded.
Searchable PDF derivatives are also excluded because Strife searches the
PostgreSQL text corpus and preserves the original file unchanged.
