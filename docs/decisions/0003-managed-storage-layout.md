# ADR 0003: Reserve `/mnt/ext/strife` for Strife Data

- **Status:** Accepted
- **Date:** 2026-07-29

## Context

The application needs non-overlapping locations for originals, upload staging, derived artifacts, and watched-folder ingestion. The user has reserved all of `/mnt/ext/strife` for the application.

## Decision

Strife owns `/mnt/ext/strife` and uses the following initial layout:

```text
/mnt/ext/strife/
  storage/
    originals/
    staging/
    derived/
  import/
    inbox/
```

`STORAGE_ROOT` points to `/mnt/ext/strife/storage` on the primary host. The importer watches `/mnt/ext/strife/import/inbox`. All paths remain configurable so development and x86-64 installations do not hard-code `/mnt/ext`.

The import inbox is outside `STORAGE_ROOT`; therefore, generated and finalized objects cannot be discovered and re-imported by the watcher.

## Alternatives Considered

- One flat directory for every object type
- A watched directory outside the Strife-owned tree
- Using display names as physical directory and file names

## Consequences

- Staging cleanup and derived-artifact cleanup have explicit boundaries.
- The application may validate that storage and import roots do not overlap incorrectly.
- Production PostgreSQL placement is not decided by this ADR; v1 development uses the Compose-managed database volume.

