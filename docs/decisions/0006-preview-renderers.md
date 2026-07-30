# ADR 0006: Preview renderers

- Status: Accepted
- Date: 2026-07-29

## Context

Strife needs deterministic DOCX-to-PDF conversion and RAW-camera thumbnails/previews on an ARM64 home server with 4 GB RAM. Tools must run out of process so timeouts and concurrency limits can contain malformed or unusually expensive files.

## Decision

Use headless LibreOffice (`soffice --headless --convert-to pdf`) for DOC/DOCX conversion. Run one isolated conversion at a time with a fresh temporary user profile and a 120-second timeout.

Use LibRaw command-line tools for RAW images. Attempt `dcraw_emu -e` first to extract the camera's embedded JPEG preview; when none exists, use `dcraw_half -w -T` for a half-resolution TIFF and encode the result with ImageMagick. Supported v1 RAW types are NEF and DNG; other LibRaw-readable types are best-effort.

## ARM64 evaluation

The tools were smoke-tested on Apple Silicon (`arm64`) with LibreOfficeDev 26.8, LibRaw's `dcraw_emu`/`dcraw_half`, and ImageMagick 7.1. Representative fixtures cover a text/table/image DOCX, a Nikon NEF, and an Adobe DNG.

| Operation | Cold/worst elapsed | Peak RSS | Result |
| --- | ---: | ---: | --- |
| DOCX → PDF | 2.1 s | 238 MB | layout, fonts, tables, and pagination retained |
| NEF embedded JPEG | 0.4 s | 34 MB | full embedded camera preview |
| DNG half decode → WebP | 2.8 s | 276 MB | correct orientation and color profile |

Malformed DOCX and truncated RAW fixtures returned non-zero without leaving a successful artifact. Output is deterministic for identical input/tool versions; generator versions include the adapter and underlying tool version.

## Alternatives

- A dedicated office service adds another long-running service without improving single-user throughput.
- Browser-side DOCX renderers have lower layout fidelity and inconsistent pagination.
- ImageMagick alone was rejected because this build has no LibRaw delegate.
- Full-resolution RAW decoding by default consumes unnecessary memory for thumbnails.

## Consequences

LibreOffice conversions are serialized. RAW extraction prefers cheap embedded previews and only decodes pixels when required. Both paths use temporary files, hard timeouts, bounded dimensions, and cached derived artifacts.
