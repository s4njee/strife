use anyhow::Result;
use strife_api::{config::Config, init_tracing, run};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let config = Config::from_env()?;
    run(config).await
}
