use std::{process::Stdio, time::Duration};

use anyhow::{Context, Result, bail};
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

const MAX_STDERR_BYTES: usize = 64 * 1024;

pub(crate) async fn run_bounded(
    mut command: Command,
    name: &str,
    process_timeout: Duration,
    max_output_bytes: usize,
) -> Result<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("start {name}"))?;
    let stdout = child
        .stdout
        .take()
        .with_context(|| format!("capture {name} stdout"))?;
    let stderr = child
        .stderr
        .take()
        .with_context(|| format!("capture {name} stderr"))?;
    let stdout_task = tokio::spawn(read_bounded(stdout, max_output_bytes));
    let stderr_task = tokio::spawn(read_bounded(stderr, MAX_STDERR_BYTES));

    let Ok(status) = timeout(process_timeout, child.wait()).await else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        bail!(
            "{name} timed out after {} seconds",
            process_timeout.as_secs()
        );
    };
    let status = status.with_context(|| format!("wait for {name}"))?;
    let stdout = stdout_task
        .await
        .with_context(|| format!("join {name} stdout reader"))??;
    let stderr = stderr_task
        .await
        .with_context(|| format!("join {name} stderr reader"))??;
    if stdout.len() > max_output_bytes {
        bail!("{name} output exceeded {max_output_bytes} bytes");
    }
    if !status.success() {
        bail!(
            "{name} exited unsuccessfully: {}",
            String::from_utf8_lossy(&stderr).trim()
        );
    }
    Ok(stdout)
}

async fn read_bounded(reader: impl tokio::io::AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>> {
    let capacity = limit.saturating_add(1);
    let mut bytes = Vec::with_capacity(capacity.min(64 * 1024));
    reader
        .take(u64::try_from(capacity)?)
        .read_to_end(&mut bytes)
        .await?;
    Ok(bytes)
}
