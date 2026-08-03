use std::env;

use anyhow::{Context, Result, bail};
use sqlx::postgres::PgPoolOptions;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();

    let database_url = env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    if database_url.trim().is_empty() {
        bail!("DATABASE_URL cannot be empty");
    }

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .context("connect to PostgreSQL")?;
    info!("applying database migrations");
    strife_db::MIGRATOR
        .run(&pool)
        .await
        .context("apply database migrations")?;
    let version = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT max(version) FROM _sqlx_migrations WHERE success",
    )
    .fetch_one(&pool)
    .await?
    .unwrap_or_default();
    info!(version, "database migrations complete");
    Ok(())
}
