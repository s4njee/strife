use std::{path::Path, time::Duration};

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::process::Command;

use crate::process::run_bounded;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// Normalized kind of one media stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamType {
    Video,
    Audio,
    Subtitle,
    Other,
}

/// One stream ready to persist in `media_streams`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamInfo {
    pub stream_index: i32,
    pub stream_type: StreamType,
    pub codec: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration_ms: Option<i64>,
    pub bitrate_bps: Option<i64>,
    pub frame_rate: Option<String>,
    pub language: Option<String>,
}

/// Complete `ffprobe` output and fields normalized for common media queries.
#[derive(Clone, Debug, PartialEq)]
pub struct FfprobeResult {
    pub container_format: Option<String>,
    pub duration_ms: Option<i64>,
    pub total_bitrate: Option<i64>,
    pub streams: Vec<StreamInfo>,
    pub warnings: Vec<String>,
    pub raw_payload: Value,
}

/// Runs `ffprobe` with the default 60-second timeout and 16 MiB safety ceiling.
///
/// # Errors
///
/// Returns an error when the process fails, times out, exceeds the ceiling, or emits bad JSON.
pub async fn extract_ffprobe(path: &Path) -> Result<FfprobeResult> {
    extract_ffprobe_with_limits(path, DEFAULT_TIMEOUT, DEFAULT_MAX_OUTPUT_BYTES).await
}

/// Runs `ffprobe` with caller-specified process limits.
///
/// # Errors
///
/// Returns an error when the process fails or its complete JSON cannot be safely returned.
pub async fn extract_ffprobe_with_limits(
    path: &Path,
    process_timeout: Duration,
    max_output_bytes: usize,
) -> Result<FfprobeResult> {
    let mut command = Command::new("ffprobe");
    command
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            "--",
        ])
        .arg(path);
    let stdout = run_bounded(command, "ffprobe", process_timeout, max_output_bytes).await?;
    parse_ffprobe_payload(serde_json::from_slice(&stdout).context("parse ffprobe JSON")?)
}

fn parse_ffprobe_payload(raw_payload: Value) -> Result<FfprobeResult> {
    let root = raw_payload
        .as_object()
        .context("ffprobe JSON was not an object")?;
    let format = root.get("format").and_then(Value::as_object);
    let container_format = format.and_then(|value| string(value.get("format_name")));
    let duration_ms = format.and_then(|value| milliseconds(value.get("duration")));
    let total_bitrate = format.and_then(|value| integer(value.get("bit_rate")));
    let streams = root
        .get("streams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .map(|stream| StreamInfo {
            stream_index: integer(stream.get("index"))
                .and_then(|value| i32::try_from(value).ok())
                .unwrap_or_default(),
            stream_type: match stream.get("codec_type").and_then(Value::as_str) {
                Some("video") => StreamType::Video,
                Some("audio") => StreamType::Audio,
                Some("subtitle") => StreamType::Subtitle,
                _ => StreamType::Other,
            },
            codec: string(stream.get("codec_name")).unwrap_or_else(|| "unknown".to_owned()),
            width: integer(stream.get("width")).and_then(|value| i32::try_from(value).ok()),
            height: integer(stream.get("height")).and_then(|value| i32::try_from(value).ok()),
            duration_ms: milliseconds(stream.get("duration")),
            bitrate_bps: integer(stream.get("bit_rate")),
            frame_rate: string(stream.get("avg_frame_rate")),
            language: stream
                .get("tags")
                .and_then(Value::as_object)
                .and_then(|tags| string(tags.get("language"))),
        })
        .collect::<Vec<_>>();
    let mut warnings = Vec::new();
    if streams.is_empty() {
        warnings.push("ffprobe returned no media streams".to_owned());
    }
    if duration_ms.is_none() {
        warnings.push("container duration is missing".to_owned());
    }
    Ok(FfprobeResult {
        container_format,
        duration_ms,
        total_bitrate,
        streams,
        warnings,
        raw_payload,
    })
}

fn string(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| match value {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn integer(value: Option<&Value>) -> Option<i64> {
    value.and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn milliseconds(value: Option<&Value>) -> Option<i64> {
    value
        .and_then(|value| value.as_f64().or_else(|| value.as_str()?.parse().ok()))
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .and_then(|seconds| format!("{:.0}", seconds * 1000.0).parse().ok())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, process::Command};

    use uuid::Uuid;

    use super::{StreamType, extract_ffprobe};

    struct Fixtures(PathBuf);

    impl Fixtures {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("strife-ffprobe-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("create fixture directory");
            Self(path)
        }

        fn generate(&self, name: &str, arguments: &[&str]) -> PathBuf {
            let path = self.0.join(name);
            let status = Command::new("ffmpeg")
                .args(["-loglevel", "error", "-y"])
                .args(arguments)
                .arg(&path)
                .status()
                .expect("run ffmpeg");
            assert!(status.success(), "generate {name}");
            path
        }
    }

    impl Drop for Fixtures {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn extracts_representative_video_and_audio_containers() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            eprintln!("ffmpeg unavailable; skipping media integration test");
            return;
        }
        let fixtures = Fixtures::new();
        let mp4 = fixtures.generate(
            "sample.mp4",
            &[
                "-f",
                "lavfi",
                "-i",
                "color=size=64x64:rate=4",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000",
                "-t",
                "0.5",
                "-c:v",
                "libx264",
                "-c:a",
                "aac",
                "-shortest",
            ],
        );
        let mkv = fixtures.generate(
            "sample.mkv",
            &[
                "-f",
                "lavfi",
                "-i",
                "color=size=64x64:rate=4",
                "-t",
                "0.5",
                "-c:v",
                "mpeg4",
            ],
        );
        let mp3 = fixtures.generate(
            "sample.mp3",
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440",
                "-t",
                "0.5",
                "-c:a",
                "mp3",
            ],
        );
        let m4a = fixtures.generate(
            "sample.m4a",
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880",
                "-t",
                "0.5",
                "-c:a",
                "aac",
            ],
        );

        let mp4_result = extract_ffprobe(&mp4).await.expect("extract MP4");
        assert!(
            mp4_result
                .streams
                .iter()
                .any(|stream| stream.codec == "h264")
        );
        assert!(
            mp4_result
                .streams
                .iter()
                .any(|stream| stream.stream_type == StreamType::Audio)
        );
        assert!(mp4_result.raw_payload.is_object());

        for path in [mkv, mp3, m4a] {
            let result = extract_ffprobe(&path)
                .await
                .expect("extract media metadata");
            assert!(!result.streams.is_empty());
            assert!(result.duration_ms.is_some());
        }
    }
}
