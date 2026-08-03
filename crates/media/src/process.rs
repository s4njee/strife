use std::{process::Stdio, time::Duration};

use anyhow::{Context, Result, bail};
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

const MAX_STDERR_BYTES: usize = 64 * 1024;

pub(crate) struct BoundedProcessOutput {
    pub stdout: Vec<u8>,
    pub peak_memory_bytes: Option<u64>,
}

pub(crate) async fn run_bounded(
    command: Command,
    name: &str,
    process_timeout: Duration,
    max_output_bytes: usize,
) -> Result<Vec<u8>> {
    Ok(
        run_bounded_with_stats(command, name, process_timeout, max_output_bytes)
            .await?
            .stdout,
    )
}

pub(crate) async fn run_bounded_with_stats(
    mut command: Command,
    name: &str,
    process_timeout: Duration,
    max_output_bytes: usize,
) -> Result<BoundedProcessOutput> {
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

    let pid = child.id();
    let mut peak_memory_bytes: Option<u64> = None;
    let wait = async {
        let mut interval = tokio::time::interval(Duration::from_millis(25));
        loop {
            tokio::select! {
                status = child.wait() => break status,
                _ = interval.tick() => {
                    if let Some(bytes) = process_memory_bytes(pid) {
                        peak_memory_bytes = Some(peak_memory_bytes.unwrap_or_default().max(bytes));
                    }
                }
            }
        }
    };
    let Ok(status) = timeout(process_timeout, wait).await else {
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
    Ok(BoundedProcessOutput {
        stdout,
        peak_memory_bytes,
    })
}

#[cfg(target_os = "linux")]
fn process_memory_bytes(pid: Option<u32>) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{}/status", pid?)).ok()?;
    let kilobytes = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    kilobytes.checked_mul(1024)
}

#[cfg(not(target_os = "linux"))]
fn process_memory_bytes(_pid: Option<u32>) -> Option<u64> {
    None
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
