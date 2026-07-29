# ADR 0002: Use Direct ZFS-Backed File Storage

- **Status:** Accepted
- **Date:** 2026-07-29

## Context

Strife needs durable storage for original files and generated artifacts on a 5 TB external HDD. The disk can be reformatted. The two candidates were direct storage on a ZFS dataset and a local MinIO service.

## Decision

The external HDD will be reformatted for ZFS, and Strife v1 will write opaque-key objects directly into a managed directory on that ZFS storage. MinIO is not part of v1.

The application will still isolate physical storage behind a Rust storage interface. PostgreSQL remains the source of truth for the virtual hierarchy and object state. Strife must refuse to start when its configured storage root is missing or unwritable.

## Alternatives Considered

- Local MinIO with an S3-compatible API
- A managed directory on a non-ZFS filesystem
- Storing file bytes in PostgreSQL

## Consequences

- v1 has fewer long-running services and lower memory overhead on the 4 GB host.
- ZFS provisioning, health, snapshots, and recovery are host-operations concerns rather than API responsibilities.
- A future MinIO or remote S3 adapter can be added without changing file-domain semantics.
- Physical keys must remain opaque and must not contain user-supplied paths.

