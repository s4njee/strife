use std::{path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use tokio::process::Command;

use crate::{detect_mime, extract_ffprobe, process::run_bounded};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThumbnailResult {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub byte_size: u64,
}

/// Generates an aspect-preserving WebP thumbnail with bounded external processes.
///
/// # Errors
/// Returns an error when decoding, resizing, probing, or output inspection fails.
pub async fn generate_thumbnail(
    source: &Path,
    dest: &Path,
    max_size: u32,
) -> Result<ThumbnailResult> {
    if max_size == 0 {
        bail!("thumbnail size must be positive")
    }
    let mime = detect_mime(source)?;
    if mime.starts_with("video/") {
        let probe = extract_ffprobe(source).await?;
        let seek_ms = probe.duration_ms.unwrap_or_default() / 10;
        let seek = format!("{}.{:03}", seek_ms / 1000, seek_ms % 1000);
        let frame = dest.with_extension("frame.png");
        let mut command = Command::new("ffmpeg");
        command
            .args(["-v", "error", "-ss", &seek, "-i"])
            .arg(source)
            .args([
                "-frames:v",
                "1",
                "-vf",
                &format!("scale={max_size}:{max_size}:force_original_aspect_ratio=decrease"),
                "-y",
            ])
            .arg(&frame);
        run_bounded(command, "ffmpeg thumbnail", Duration::from_secs(30), 1024).await?;
        let mut encode = Command::new("magick");
        encode.arg(&frame).args(["-quality", "82"]).arg(dest);
        let encoded = run_bounded(encode, "WebP encode", Duration::from_secs(30), 1024).await;
        let _ = tokio::fs::remove_file(frame).await;
        encoded?;
    } else {
        let input = if is_raw(&mime) {
            let mut raw = Command::new("dcraw_emu");
            raw.args(["-e", "-c"]).arg(source);
            let bytes = run_bounded(
                raw,
                "LibRaw preview",
                Duration::from_secs(30),
                64 * 1024 * 1024,
            )
            .await?;
            let extracted = dest.with_extension("embedded-preview");
            tokio::fs::write(&extracted, bytes).await?;
            extracted
        } else {
            source.to_path_buf()
        };
        let input_arg = if mime == "image/gif" {
            format!("{}[0]", input.display())
        } else {
            input.display().to_string()
        };
        let mut command = Command::new("magick");
        command
            .arg(input_arg)
            .args([
                "-auto-orient",
                "-thumbnail",
                &format!("{max_size}x{max_size}>"),
                "-quality",
                "82",
            ])
            .arg(dest);
        let result = run_bounded(
            command,
            "ImageMagick thumbnail",
            Duration::from_secs(30),
            1024,
        )
        .await;
        if input != source {
            let _ = tokio::fs::remove_file(input).await;
        }
        result?;
    }
    let mut identify = Command::new("magick");
    identify.args(["identify", "-format", "%w %h"]).arg(dest);
    let dimensions = String::from_utf8(
        run_bounded(identify, "thumbnail identify", Duration::from_secs(5), 1024).await?,
    )?;
    let mut parts = dimensions.split_whitespace();
    let width = parts.next().context("missing thumbnail width")?.parse()?;
    let height = parts.next().context("missing thumbnail height")?.parse()?;
    Ok(ThumbnailResult {
        width,
        height,
        format: "image/webp".to_owned(),
        byte_size: tokio::fs::metadata(dest).await?.len(),
    })
}

fn is_raw(mime: &str) -> bool {
    matches!(
        mime,
        "image/x-nikon-nef" | "image/x-adobe-dng" | "image/x-dcraw"
    )
}

/// Generates a bandwidth-bounded WebP image preview, including RAW embedded previews.
///
/// # Errors
/// Returns an error when the source cannot be decoded or the preview cannot be written.
pub async fn generate_image_preview(source: &Path, dest: &Path) -> Result<ThumbnailResult> {
    generate_thumbnail(source, dest, 2048).await
}

#[cfg(test)]
mod tests {
    use super::generate_thumbnail;
    use std::{fs, process::Command};
    use uuid::Uuid;

    #[tokio::test]
    async fn thumbnails_images_gif_and_video() {
        if Command::new("magick").arg("-version").output().is_err() {
            return;
        }
        let root = std::env::temp_dir().join(format!("strife-thumbs-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        for (name, color) in [
            ("sample.jpg", "red"),
            ("sample.png", "green"),
            ("sample.gif", "blue"),
        ] {
            assert!(
                Command::new("magick")
                    .args(["-size", "640x320", &format!("xc:{color}")])
                    .arg(root.join(name))
                    .status()
                    .unwrap()
                    .success()
            );
            let result =
                generate_thumbnail(&root.join(name), &root.join(format!("{name}.webp")), 256)
                    .await
                    .unwrap();
            assert_eq!((result.width, result.height), (256, 128));
        }
        if Command::new("ffmpeg").arg("-version").output().is_ok() {
            let video = root.join("sample.mp4");
            assert!(
                Command::new("ffmpeg")
                    .args([
                        "-v",
                        "error",
                        "-f",
                        "lavfi",
                        "-i",
                        "color=size=640x320:duration=1",
                        "-y"
                    ])
                    .arg(&video)
                    .status()
                    .unwrap()
                    .success()
            );
            assert_eq!(
                generate_thumbnail(&video, &root.join("video.webp"), 256)
                    .await
                    .unwrap()
                    .width,
                256
            );
        }
        let _ = fs::remove_dir_all(root);
    }
}
