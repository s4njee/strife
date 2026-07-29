# Strife — Active v1 Questions

Only unresolved questions that can affect v1 belong here. Settled decisions are recorded in [`README.md`](README.md); questions deliberately postponed until v2 or later are in [`deferred.md`](deferred.md).

## Milestone 3 — Watched-Folder Import

### Does importing copy or move a source file?

Copying leaves the source intact but temporarily needs space for two copies. Moving consumes less space but is destructive and may not be atomic across filesystems.

### How is the watched path mapped into Strife?

Decide whether there is one fixed watch path and destination, or configurable source-to-destination mappings. Also decide whether the watch directory itself appears as a folder or only its contents are imported.

### When is a watched file considered stable and ready to import?

Possible signals include unchanged size/modification time across scans, a minimum quiet period, or an explicit producer-side rename into place.

### What happens after an import succeeds?

For copied sources, choose whether to leave them, move them to an archive/completed directory, or delete them. Any destructive policy must be explicit.

### What happens when an already imported source changes or disappears?

Choose whether changes are ignored, rejected and reported, imported as a new conflict, or update the existing Strife file. v1 has no versions and should not silently overwrite an existing node.

### How should source-name conflicts be handled?

The settled default is to reject duplicates. Decide whether the importer keeps retrying, quarantines the source, or records one persistent actionable error until the conflict is resolved.

## Milestone 4 — Metadata

### How much raw extractor output should PostgreSQL retain?

Options include:

- keep all raw JSON up to a per-record size cap;
- retain a selected subset and normalized fields;
- store large raw results as derived sidecars outside PostgreSQL;
- truncate oversized fields with a recorded warning.

The choice should be measured with real ExifTool, ffprobe, and Tika fixtures on the expected library.

### Which metadata needs typed columns?

At minimum, consider detected MIME, media kind, duration, width, height, capture time, page count, bitrate, codec, orientation, and GPS availability. Only fields used frequently for display, sorting, or filtering need first-class columns; uncommon data can remain in versioned JSON.

### Which additional “common” formats must pass the v1 acceptance suite?

The baseline is DOC, DOCX, PDF, JPEG, GIF, PNG, MP4, MKV, MP3, M4A, MOV, plus common formats recognized by the chosen tools and raw camera images. Create an explicit test matrix so “all common formats” has a verifiable boundary.

## Milestone 5 — Office and Raw Previews

### Which tool should render DOCX and other supported office files?

Options to evaluate on ARM64 include headless LibreOffice and a dedicated conversion service. Compare fidelity, cold-start time, memory use, package/container size, malformed-input behavior, and output determinism.

### Which tool should decode raw camera images for thumbnails and previews?

Evaluate an ARM64-available libraw-based option using representative camera files. Metadata extraction alone does not guarantee preview decoding.

## Milestone 6 — Command Bar

### Which filesystem-like commands are in v1?

Candidate commands are `pwd`, `ls`, `cd`, `mkdir`, `mv`, `rm`, `restore`, and `open`. Decide which map cleanly to existing UI operations.

### How shell-like should parsing be?

Define quoting, escaping, relative and absolute virtual paths, autocomplete, command history, error formatting, and whether destructive commands require confirmation. It should resemble a filesystem command interface without attempting to implement a general shell.

## Decision Recording

When an answer materially affects the architecture, add a short record under `docs/decisions/` with the context, decision, alternatives, consequences, and date. Remove the question from this file after `README.md` reflects the decision.
