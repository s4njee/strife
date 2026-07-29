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

Compile-time checked SQL queries are cached under `.sqlx/`, allowing CI to compile with no live database. After adding or changing a checked query, start PostgreSQL, apply migrations, then run:

```sh
cargo sqlx prepare --workspace -- --all-targets
```

Validate the cache without a database:

```sh
SQLX_OFFLINE=true cargo check --workspace --all-targets
```
