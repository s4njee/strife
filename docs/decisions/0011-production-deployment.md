# ADR 0011: Single-host production deployment

- **Status:** Accepted
- **Date:** 2026-08-03

## Context

Strife targets one trusted LAN host with ZFS-backed canonical storage. The
application needs reproducible migrations, bounded shutdown, and rollback, but
does not need multi-host scheduling or a public control plane.

## Decision

- Production uses Docker Compose for PostgreSQL, Tika, the one-shot migrator,
  API, worker, and Caddy-served web application.
- systemd owns the Compose lifecycle on Orion; immutable image tags and revision
  labels identify every rollout.
- Caddy serves static assets and proxies the private API. No product TLS or
  authentication boundary is claimed; the deployment remains LAN-only.
- PostgreSQL migrations run once through `strife-migrate`. API and worker
  startup use `RUN_MIGRATIONS=false` in production.
- Canonical storage and PostgreSQL live in separate host datasets. Secrets live
  under `/etc/strife`, never in the checkout.
- Rollback restores prior images while normally retaining additive schema.
  Destructive down migrations require a separate reviewed recovery operation.

## Consequences

Compose plus systemd is understandable and sufficient for one host, and it
keeps data paths explicit. It does not provide high availability, automatic
off-host backup, public TLS, or multi-node scheduling. Kubernetes, published
registry images, native OS packages, and public-internet hardening remain
deferred until the single-host operating evidence justifies their complexity.
