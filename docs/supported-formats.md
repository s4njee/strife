# Supported Formats (v1)

## Metadata extraction

| Format family | Examples | Primary tools | Notes |
|---|---|---|---|
| Documents | PDF, DOC, DOCX | Apache Tika | Text/title/author when available |
| Images | JPEG, PNG, GIF, WebP | libmagic, ExifTool | Dimensions, orientation, GPS, camera |
| Camera raw | NEF, DNG | ExifTool, LibRaw (previews) | Other LibRaw types best-effort |
| Video | MP4, MKV, MOV | ffprobe | Streams, duration, codecs |
| Audio | MP3, M4A | ffprobe | Duration, codecs |
| Everything else | * | libmagic | Generic MIME + generic metadata record |

MIME is detected from **content bytes**, not extensions.

## Previews and thumbnails

| Kind | Preview | Thumbnail | Transcode? |
|---|---|---|---|
| JPEG/PNG/GIF/WebP | Browser-native and/or cached | ~256×256 cached | No |
| RAW (NEF/DNG) | Generated via LibRaw path | Generated | No |
| PDF | Browser-native | Generated when requested | No |
| DOCX | Converted to PDF via LibreOffice, then previewed | Via PDF path | No |
| Audio | Browser-native when codec supported | — | No |
| Video | Browser-native when codec supported | Frame grab when generated | **No** (unsupported codecs download only) |

## Explicitly not extracted in v1

- OCR / handwriting
- Cover art, waveforms, embedded document attachments
- Archive listings
- Editable metadata writes

See [known-limitations.md](known-limitations.md) and ADR 0005 / 0006.
