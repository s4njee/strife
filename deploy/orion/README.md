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
