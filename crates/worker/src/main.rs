use std::sync::Arc;

use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use strife_db::MIGRATOR;
use strife_storage::LocalFsBackend;
use strife_worker::{UnavailableMetadataHandler, WorkerConfig, init_tracing, run};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let config = WorkerConfig::from_env()?;
    let max_connections = u32::try_from(config.concurrency)?.saturating_add(2);
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(&config.database_url)
        .await?;
    MIGRATOR.run(&pool).await?;
    let storage = Arc::new(LocalFsBackend::new(&config.storage_root).await?);

    run(config, pool, storage, Arc::new(UnavailableMetadataHandler)).await
}
