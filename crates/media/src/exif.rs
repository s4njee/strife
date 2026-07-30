use std::{path::Path, process::Stdio, time::Duration};

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
// This is a fail-fast process safety boundary, not a storage truncation limit.
const DEFAULT_MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;

/// Complete `ExifTool` output plus fields normalized for common queries.
#[derive(Clone, Debug, PartialEq)]
pub struct ExifResult {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub orientation: Option<i32>,
    pub capture_time: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub color_space: Option<String>,
    pub warnings: Vec<String>,
    pub raw_payload: Value,
}

/// Runs `ExifTool` with the default 30-second timeout and 16 MiB safety ceiling.
///
/// # Errors
///
/// Returns an error when `ExifTool` cannot run, times out, exceeds its safety ceiling, exits
/// unsuccessfully, or emits malformed JSON. Output is never truncated and treated as successful.
pub async fn extract_exif(path: &Path) -> Result<ExifResult> {
    extract_exif_with_limits(path, DEFAULT_TIMEOUT, DEFAULT_MAX_OUTPUT_BYTES).await
}

/// Runs `ExifTool` with explicit process limits, primarily for controlled deployments and tests.
///
/// # Errors
///
/// Returns an error for process or JSON failures, including output larger than `max_output_bytes`.
pub async fn extract_exif_with_limits(
    path: &Path,
    process_timeout: Duration,
    max_output_bytes: usize,
) -> Result<ExifResult> {
    let mut child = Command::new("exiftool")
        .args(["-json", "-n", "--"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("start ExifTool")?;
    let stdout = child.stdout.take().context("capture ExifTool stdout")?;
    let stderr = child.stderr.take().context("capture ExifTool stderr")?;
    let stdout_task = tokio::spawn(read_bounded(stdout, max_output_bytes));
    let stderr_task = tokio::spawn(read_bounded(stderr, MAX_STDERR_BYTES));

    let Ok(status) = timeout(process_timeout, child.wait()).await else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        bail!(
            "ExifTool timed out after {} seconds",
            process_timeout.as_secs()
        );
    };
    let status = status.context("wait for ExifTool")?;
    let stdout = stdout_task.await.context("join ExifTool stdout reader")??;
    let stderr = stderr_task.await.context("join ExifTool stderr reader")??;
    if stdout.len() > max_output_bytes {
        bail!("ExifTool output exceeded {max_output_bytes} bytes");
    }
    if !status.success() {
        bail!(
            "ExifTool exited unsuccessfully: {}",
            String::from_utf8_lossy(&stderr).trim()
        );
    }
    parse_exif_payload(serde_json::from_slice(&stdout).context("parse ExifTool JSON")?)
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

fn parse_exif_payload(raw_payload: Value) -> Result<ExifResult> {
    let metadata = raw_payload
        .as_array()
        .and_then(|records| records.first())
        .and_then(Value::as_object)
        .context("ExifTool JSON did not contain a metadata object")?;
    let width = integer(
        metadata,
        &["ImageWidth", "ExifImageWidth", "SourceImageWidth"],
    );
    let height = integer(
        metadata,
        &["ImageHeight", "ExifImageHeight", "SourceImageHeight"],
    );
    let orientation = integer(metadata, &["Orientation"]);
    let capture_time = text(
        metadata,
        &["DateTimeOriginal", "CreateDate", "MediaCreateDate"],
    );
    let camera_make = text(metadata, &["Make"]);
    let camera_model = text(metadata, &["Model"]);
    let gps_latitude = number(metadata, &["GPSLatitude"]);
    let gps_longitude = number(metadata, &["GPSLongitude"]);
    let color_space = text(metadata, &["ColorSpace", "ICC_Profile:ColorSpaceData"]);

    let mut warnings = Vec::new();
    if width.is_none() || height.is_none() {
        warnings.push("image dimensions are missing".to_owned());
    } else if width == Some(0) || height == Some(0) {
        warnings.push("image dimensions are zero".to_owned());
    }
    if gps_latitude.is_some() != gps_longitude.is_some() {
        warnings.push("only one GPS coordinate is present".to_owned());
    }
    if orientation.is_some_and(|value| !(1..=8).contains(&value)) {
        warnings.push("orientation is outside the EXIF range 1 through 8".to_owned());
    }

    Ok(ExifResult {
        width,
        height,
        orientation,
        capture_time,
        camera_make,
        camera_model,
        gps_latitude,
        gps_longitude,
        color_space,
        warnings,
        raw_payload,
    })
}

fn value<'a>(metadata: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| metadata.get(*key))
}

fn text(metadata: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    value(metadata, keys).and_then(|value| match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn number(metadata: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    value(metadata, keys).and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn integer(metadata: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<i32> {
    value(metadata, keys).and_then(|value| {
        value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use serde_json::json;
    use uuid::Uuid;

    use super::{extract_exif, parse_exif_payload};

    struct Fixtures(PathBuf);

    impl Fixtures {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("strife-exif-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("create fixture directory");
            Self(path)
        }

        fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, bytes).expect("write fixture");
            path
        }
    }

    impl Drop for Fixtures {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn extracts_jpeg_and_png_payloads_without_discarding_json() {
        if std::process::Command::new("exiftool")
            .arg("-ver")
            .status()
            .is_err()
        {
            eprintln!("ExifTool unavailable; skipping adapter integration test");
            return;
        }
        let fixtures = Fixtures::new();
        let jpeg = fixtures.write(
            "sample.jpg",
            b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00\xff\xd9",
        );
        let png = fixtures.write(
            "sample.png",
            b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde",
        );
        for path in [jpeg, png] {
            let result = extract_exif(&path).await.expect("extract image metadata");
            assert!(result.raw_payload.is_array());
            assert!(result.raw_payload[0]["SourceFile"].is_string());
        }
    }

    #[test]
    fn normalizes_representative_raw_camera_metadata() {
        let payload = json!([{
            "SourceFile": "sample.nef",
            "FileType": "NEF",
            "ExifImageWidth": 8256,
            "ExifImageHeight": 5504,
            "Orientation": 6,
            "DateTimeOriginal": "2025:04:03 12:30:45",
            "Make": "NIKON CORPORATION",
            "Model": "NIKON Z 8",
            "GPSLatitude": 41.8819,
            "GPSLongitude": -87.6278,
            "ColorSpace": "sRGB",
            "MakerNotes": {"Retained": true}
        }]);
        let result = parse_exif_payload(payload.clone()).expect("parse raw metadata");
        assert_eq!(result.width, Some(8256));
        assert_eq!(result.camera_model.as_deref(), Some("NIKON Z 8"));
        assert_eq!(result.gps_longitude, Some(-87.6278));
        assert_eq!(result.raw_payload, payload);
        assert!(result.warnings.is_empty());
    }
}
