# ADR 0004: Move Files from a Fixed, Manually Scanned Inbox

- **Status:** Accepted
- **Date:** 2026-07-29

## Context

Strife v1 needs predictable watched-folder behavior without destructive surprises, duplicate imports, or a background scanner reading files that another process is still writing. The deployment is single-user and has one agreed server-side inbox.

## Decision

- The sole v1 import source is `/mnt/ext/watch`; its contents map into Strife's root folder while preserving their relative hierarchy.
- Scans run only when the user explicitly requests one. There is no polling interval or continuous watcher in v1.
- A manual scan enqueues a durable background job. The worker records each regular, non-hidden file and immediately imports it before continuing the tree walk; special files are ignored. A file is eligible when its size and modification time remain unchanged while it is streamed into staging; a changed file is returned to `discovered` for a later pass.
- Import-scan jobs renew their leases while running. If a worker stops, the expired lease returns to the queue and the next attempt safely revisits persisted entries while continuing remaining files.
- Import uses move semantics: after the managed original and database records are finalized successfully, Strife removes the source file and then prunes empty source directories. A failure leaves the source in place.
- Once a source has moved out of the inbox, Strife does not monitor its former path. A later file placed at that path is a new import attempt, but v1 never overwrites an existing node.
- Name conflicts and other failures become persistent import-entry errors. They do not block unrelated files and can be retried after the user resolves the cause.

## Alternatives Considered

- Copy sources and leave them in place
- Continuously poll and infer stability across timed scans
- Support multiple configurable source-to-destination mappings
- Automatically rename or overwrite conflicts

## Consequences

- The producer or user controls readiness by requesting a scan only after writes are complete.
- Import frees inbox capacity after success and never deletes a source before durable finalization.
- The v1 management API exposes status, manual scan, enable/disable, failed entries, and retry; it does not create arbitrary sources.
- Multiple sources, configurable paths and destinations, scheduled scans, and continuous watching are deferred to v2 or later.
