# Supported Formats (v1)

## Metadata extraction

| Format family   | Examples                                   | Primary tools      | OCR input             | Notes                                                              |
| --------------- | ------------------------------------------ | ------------------ | --------------------- | ------------------------------------------------------------------ |
| Documents       | PDF, DOC, DOCX                             | Apache Tika        | PDF                   | Embedded PDF text takes precedence; image-only pages use Tesseract |
| Images          | JPEG, PNG, GIF, WebP, TIFF                 | libmagic, ExifTool | JPEG, PNG, WebP, TIFF | Multi-page TIFF is preserved; GIF is not an OCR input              |
| Camera raw      | NEF, DNG, CR2/CR3, ARW, RW2, RAF, ORF, PEF | ExifTool, LibRaw   | Yes                   | Uses the same LibRaw embedded-preview path as previews             |
| Video           | MP4, MKV, MOV                              | ffprobe            | No                    | Streams, duration, codecs                                          |
| Audio           | MP3, M4A                                   | ffprobe            | No                    | Duration, codecs                                                   |
| Everything else | *                                          | libmagic           | No                    | Recorded as OCR `unsupported`                                      |

MIME is detected from **content bytes**, not extensions.

## Previews and thumbnails

| Kind              | Preview                                          | Thumbnail                 | Transcode?                                |
| ----------------- | ------------------------------------------------ | ------------------------- | ----------------------------------------- |
| JPEG/PNG/GIF/WebP | Browser-native and/or cached                     | ~256×256 cached           | No                                        |
| RAW (NEF/DNG)     | Generated via LibRaw path                        | Generated                 | No                                        |
| PDF               | Browser-native                                   | Generated when requested  | No                                        |
| DOCX              | Converted to PDF via LibreOffice, then previewed | Via PDF path              | No                                        |
| Audio             | Browser-native when codec supported              | —                         | No                                        |
| Video             | Browser-native when codec supported              | Frame grab when generated | **No** (unsupported codecs download only) |

## Explicitly not extracted in v1

- Handwriting recognition
- Cover art, waveforms, embedded document attachments
- Archive listings
- Editable metadata writes

See [known-limitations.md](known-limitations.md) and ADR 0005 / 0006.
