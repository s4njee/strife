# Development Database

PostgreSQL is configured exclusively through `DATABASE_URL`. The development value and its component Compose variables are documented in `.env.example`.

## Start PostgreSQL

```sh
docker compose -f docker-compose.dev.yml up -d postgres
```

## Apply and Revert Migrations

Run migrations from the repository root:

```sh
cargo sqlx migrate run --source crates/db/migrations
cargo sqlx migrate revert --source crates/db/migrations
```

The migration files are embedded into `strife-db` at compile time. Application startup applies pending migrations before accepting requests.

## Refresh Offline Query Metadata

Compile-time checked SQL queries are cached under `.sqlx/`, allowing CI to compile with no live database. After adding or changing a checked query, start PostgreSQL and run:

```sh
make sqlx-prepare
```

This applies every migration before preparing the cache. Check that the cache
and the reviewed runtime-query inventory are current with:

```sh
make sqlx-check
make sqlx-inventory-check
```

Runtime queries are exceptions governed by [ADR 0010](../decisions/0010-sqlx-compile-time-query-policy.md)
and listed individually in [`sqlx-runtime-queries.md`](sqlx-runtime-queries.md).

Validate the cache without a database:

```sh
SQLX_OFFLINE=true cargo check --workspace --all-targets
```
