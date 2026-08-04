# Orion deployment

This directory contains the systemd wrapper for the Debian 13 ARM64 container
deployment used by `orion.local`.

- PostgreSQL, Apache Tika, the API, worker, and frontend all run through the
  production Docker Compose configuration at the repository root.
- Only the frontend container publishes a host port. PostgreSQL, Tika, and the
  API are reachable only over the private Compose network.
- The API and worker images run as the unprivileged UID/GID `10001` with a
  read-only root filesystem and dropped Linux capabilities.
- Caddy serves the production SolidJS bundle and proxies `/api/*` to Axum over
  the private Compose network.
- `/srv/strife/storage` and `/srv/strife/import` inherit the 1 MiB record size
  intended for large files; `/srv/strife/postgres` is a child ZFS dataset with
  a 16 KiB record size.

## Importing iCloud

Run `scripts/import-icloud.sh` on a Mac to copy a materialized iCloud Drive
tree or an `osxphotos` export into Orion's ZFS-backed inbox and start Strife's
manual import pass:

```sh
scripts/import-icloud.sh \
  "$HOME/Library/Mobile Documents/com~apple~CloudDocs" icloud-drive
```

Use `--dry-run` to inspect the source and destination without contacting
Orion, or `--stage-only` to transfer files and start the scan later from the
Imports page. The script skips hidden Apple metadata and cloud-only
`*.icloud` stubs; materialize those stubs and rerun to pick them up.

The database secret lives only on Orion in `/etc/strife/postgres.env`. Image
revision metadata lives in `/etc/strife/revision.env`. Neither is checked into
the repository.

## Host layout and secrets

```text
/opt/strife                 checked-out deployment revision
/srv/strife/postgres        PostgreSQL dataset (16 KiB record size)
/srv/strife/storage         canonical originals and derived storage
/srv/strife/import          configured import inbox
/etc/strife/postgres.env    POSTGRES_PASSWORD (root-readable only)
/etc/strife/revision.env    immutable image tag/revision and backfill gate
```

Create the secret files with mode `0600`. Repository ignore rules reject every
`.env` file except `.env.example`; the `/etc/strife` files never belong in the
checkout.

## Install

1. Create the datasets/directories above and grant storage/import ownership to
   UID/GID `10001`; PostgreSQL owns its own dataset inside the container.
2. Clone the repository at `/opt/strife` and check out a reviewed immutable tag.
3. Write `POSTGRES_PASSWORD` to `/etc/strife/postgres.env`; write
   `STRIFE_IMAGE_TAG`, `STRIFE_REVISION`, and `BACKFILL_ENABLED=false` to
   `/etc/strife/revision.env`. Turning the gate on never resumes a paused
   campaign by itself.
4. Build or load the four images for that revision, then install the unit:

   ```sh
   sudo install -m 0644 deploy/orion/strife.service /etc/systemd/system/
   sudo systemctl daemon-reload
   sudo systemctl enable --now strife.service
   docker compose --env-file /etc/strife/postgres.env \
     -f docker-compose.prod.yml ps
   ```

## Upgrade

1. Record the current revision and image digests; take and verify the database
   backup and originals snapshot described in `docs/backfill.md`.
2. Fetch and check out the reviewed tag, build/load its immutable images, and
   update `/etc/strife/revision.env`.
3. Keep `BACKFILL_ENABLED=false` in `/etc/strife/revision.env`, pause any
   campaign, and let leased work drain.
4. Run the one-shot `migrate` service, then reload the stack:

   ```sh
   docker compose --env-file /etc/strife/postgres.env \
     -f docker-compose.prod.yml run --rm migrate
   sudo systemctl reload strife.service
   curl -fsS http://127.0.0.1/api/ready
   ```

5. Verify foreground upload, download, import, OCR, and email behavior before
   enabling any historical coordinator.

## Rollback

Pause campaigns and drain leased work first. Restore the previous immutable
image tag/revision and reload the unit. Additive schema normally remains in
place; never run destructive down migrations automatically. If migration or
data verification failed, keep the worker stopped and follow the rehearsed
PostgreSQL/storage restore procedure before serving writes. Confirm readiness,
ordinary file access, and foreground queues after rollback.
