use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use tokio::process::Command;

use crate::process::{run_bounded, run_bounded_with_stats};

/// One page recognized by Tesseract.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrPage {
    pub page_number: i32,
    pub content: String,
    pub confidence: Option<f32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

/// Typed OCR output persisted by the worker.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrResult {
    pub pages: Vec<OcrPage>,
    pub language: String,
    pub engine_version: String,
    pub warnings: Vec<String>,
    pub peak_memory_bytes: Option<u64>,
}

/// Configurable rasterization boundaries applied before Tesseract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OcrNormalizationLimits {
    pub raster_dpi: u32,
    pub max_pages: u32,
    pub max_pixels_per_page: u64,
    pub memory_limit_bytes: u64,
    pub process_timeout: Duration,
}

/// One normalized raster page in source order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedOcrPage {
    pub page_number: i32,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

/// Managed normalized pages. Dropping this value removes every intermediate page,
/// including during unwinding.
#[derive(Debug)]
pub struct NormalizedOcrInput {
    root: PathBuf,
    pub pages: Vec<NormalizedOcrPage>,
}

impl Drop for NormalizedOcrInput {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Returns whether a detected MIME type belongs to the initial OCR input matrix.
#[must_use]
pub fn is_supported_ocr_mime(mime: &str) -> bool {
    mime == "application/pdf"
        || matches!(
            mime,
            "image/jpeg" | "image/png" | "image/tiff" | "image/webp"
        )
        || is_raw_mime(mime)
}

/// Rasterizes a supported OCR input into ordered PNG pages under a managed temporary path.
///
/// PDF pages use Poppler, TIFF frames use `ImageMagick`, and RAW files use the same `LibRaw`
/// embedded preview path as preview generation before `ImageMagick` normalization.
///
/// # Errors
///
/// Returns a specific error when the format is unsupported or a configured page/pixel boundary
/// is exceeded, or when an external normalizer fails.
pub async fn normalize_ocr_input(
    source: &Path,
    mime: &str,
    limits: OcrNormalizationLimits,
) -> Result<NormalizedOcrInput> {
    if !is_supported_ocr_mime(mime) {
        bail!("OCR does not support MIME type {mime}");
    }
    let root = std::env::temp_dir().join(format!("strife-ocr-pages-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir(&root)
        .await
        .context("create managed OCR page directory")?;
    let result = rasterize(source, mime, &root, limits).await;
    match result {
        Ok(pages) => Ok(NormalizedOcrInput { root, pages }),
        Err(error) => {
            let _ = tokio::fs::remove_dir_all(root).await;
            Err(error)
        }
    }
}

async fn rasterize(
    source: &Path,
    mime: &str,
    root: &Path,
    limits: OcrNormalizationLimits,
) -> Result<Vec<NormalizedOcrPage>> {
    if limits.max_pages == 0 || limits.max_pixels_per_page == 0 || limits.raster_dpi == 0 {
        bail!("OCR normalization limits must be positive");
    }
    let memory_limit = limits.memory_limit_bytes.to_string();
    if mime == "application/pdf" {
        let page_count = pdf_page_count(source, limits.process_timeout).await?;
        if page_count > limits.max_pages {
            bail!(
                "OCR page limit exceeded: {page_count} pages is greater than {}",
                limits.max_pages
            );
        }
        let mut command = Command::new("pdftoppm");
        command
            .args(["-png", "-r", &limits.raster_dpi.to_string()])
            .arg(source)
            .arg(root.join("page"));
        run_bounded(
            command,
            "PDF OCR rasterization",
            limits.process_timeout,
            4096,
        )
        .await?;
    } else {
        let input = if is_raw_mime(mime) {
            let mut raw = Command::new("dcraw_emu");
            raw.args(["-e", "-c"]).arg(source);
            let bytes = run_bounded(
                raw,
                "LibRaw OCR preview",
                limits.process_timeout,
                usize::try_from(limits.memory_limit_bytes)?,
            )
            .await?;
            let preview = root.join("raw-preview");
            tokio::fs::write(&preview, bytes).await?;
            preview
        } else {
            source.to_path_buf()
        };
        let mut command = Command::new("magick");
        command
            .env("MAGICK_MEMORY_LIMIT", &memory_limit)
            .env("MAGICK_MAP_LIMIT", &memory_limit)
            .arg(&input)
            .arg("-auto-orient")
            .arg(root.join("page-%06d.png"));
        run_bounded(
            command,
            "OCR image normalization",
            limits.process_timeout,
            4096,
        )
        .await?;
    }
    let mut paths = Vec::new();
    let mut entries = tokio::fs::read_dir(root).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("png") {
            paths.push(path);
        }
    }
    paths.sort();
    let page_count = u32::try_from(paths.len()).context("OCR page count exceeds u32")?;
    if page_count == 0 {
        bail!("OCR normalization produced no pages");
    }
    if page_count > limits.max_pages {
        bail!(
            "OCR page limit exceeded: {page_count} pages is greater than {}",
            limits.max_pages
        );
    }
    let mut pages = Vec::with_capacity(paths.len());
    for (index, path) in paths.into_iter().enumerate() {
        let (width, height) = image_dimensions(&path, limits.process_timeout).await?;
        let pixels = u64::from(width).saturating_mul(u64::from(height));
        if pixels > limits.max_pixels_per_page {
            bail!(
                "OCR pixel limit exceeded on page {}: {pixels} pixels is greater than {}",
                index + 1,
                limits.max_pixels_per_page
            );
        }
        pages.push(NormalizedOcrPage {
            page_number: i32::try_from(index + 1)?,
            path,
            width,
            height,
        });
    }
    Ok(pages)
}

async fn pdf_page_count(source: &Path, process_timeout: Duration) -> Result<u32> {
    let mut command = Command::new("pdfinfo");
    command.arg(source);
    let output = String::from_utf8(
        run_bounded(command, "PDF page count", process_timeout, 64 * 1024).await?,
    )?;
    output
        .lines()
        .find_map(|line| line.strip_prefix("Pages:"))
        .map(str::trim)
        .context("pdfinfo did not report a page count")?
        .parse()
        .context("parse PDF page count")
}

async fn image_dimensions(path: &Path, process_timeout: Duration) -> Result<(u32, u32)> {
    let mut command = Command::new("magick");
    command.args(["identify", "-format", "%w %h"]).arg(path);
    let output = String::from_utf8(
        run_bounded(command, "OCR page dimensions", process_timeout, 1024).await?,
    )?;
    let mut parts = output.split_whitespace();
    Ok((
        parts.next().context("missing OCR page width")?.parse()?,
        parts.next().context("missing OCR page height")?.parse()?,
    ))
}

fn is_raw_mime(mime: &str) -> bool {
    matches!(
        mime,
        "image/x-canon-cr2"
            | "image/x-canon-cr3"
            | "image/x-nikon-nef"
            | "image/x-sony-arw"
            | "image/x-adobe-dng"
            | "image/x-panasonic-rw2"
            | "image/x-fuji-raf"
            | "image/x-olympus-orf"
            | "image/x-pentax-pef"
            | "image/x-dcraw"
    )
}

#[derive(Default)]
struct PageBuilder {
    width: Option<i32>,
    height: Option<i32>,
    lines: BTreeMap<(i32, i32, i32), Vec<String>>,
    weighted_confidence: f64,
    confidence_characters: u32,
}

/// Confirms that the configured Tesseract executable is available and returns its version.
///
/// # Errors
///
/// Returns an actionable startup error when the binary cannot execute or report a version.
pub async fn verify_tesseract(binary: &str) -> Result<String> {
    let mut command = Command::new(binary);
    command.arg("--version");
    let output = run_bounded(command, "Tesseract version check", Duration::from_secs(5), 4096)
        .await
        .with_context(|| {
            format!(
                "Tesseract is unavailable at '{binary}'; install tesseract-ocr and the eng language pack or set TESSERACT_BIN"
            )
        })?;
    let first_line = String::from_utf8(output)?
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if first_line.is_empty() {
        bail!("Tesseract version check returned no version")
    }
    Ok(first_line)
}

/// Runs Tesseract TSV extraction through the shared bounded-process helper.
///
/// Confidence is weighted by recognized word character count. Words with negative confidence
/// are excluded, matching ADR 0008.
///
/// # Errors
///
/// Returns an error when Tesseract fails, times out, exceeds the output ceiling, or emits an
/// invalid TSV payload.
pub async fn extract_ocr(
    path: &Path,
    binary: &str,
    language: &str,
    engine_version: &str,
    process_timeout: Duration,
    max_output_bytes: usize,
) -> Result<OcrResult> {
    let mut command = Command::new(binary);
    command
        .arg(path)
        .arg("stdout")
        .args(["-l", language, "tsv"])
        .env("OMP_THREAD_LIMIT", "1");
    let output =
        run_bounded_with_stats(command, "Tesseract OCR", process_timeout, max_output_bytes).await?;
    let mut result = parse_tsv(&String::from_utf8(output.stdout)?, language, engine_version)?;
    result.peak_memory_bytes = output.peak_memory_bytes;
    Ok(result)
}

fn parse_tsv(tsv: &str, language: &str, engine_version: &str) -> Result<OcrResult> {
    let mut pages = BTreeMap::<i32, PageBuilder>::new();
    for (index, row) in tsv.lines().enumerate() {
        if index == 0 && row.starts_with("level\t") {
            continue;
        }
        if row.trim().is_empty() {
            continue;
        }
        let fields = row.splitn(12, '\t').collect::<Vec<_>>();
        if fields.len() != 12 {
            bail!(
                "Tesseract TSV row {} has {} columns, expected 12",
                index + 1,
                fields.len()
            );
        }
        let level = parse_i32(fields[0], index)?;
        let page_number = parse_i32(fields[1], index)?;
        if page_number < 1 {
            bail!("Tesseract TSV row {} has an invalid page number", index + 1);
        }
        let page = pages.entry(page_number).or_default();
        if level == 1 {
            page.width = Some(parse_i32(fields[8], index)?);
            page.height = Some(parse_i32(fields[9], index)?);
        }
        if level != 5 || fields[11].trim().is_empty() {
            continue;
        }
        let block = parse_i32(fields[2], index)?;
        let paragraph = parse_i32(fields[3], index)?;
        let line = parse_i32(fields[4], index)?;
        let word = fields[11].trim().to_owned();
        let characters = word.chars().count();
        let confidence_characters = u32::try_from(characters)
            .context("recognized OCR word exceeds the confidence weighting range")?;
        let confidence = fields[10]
            .parse::<f64>()
            .with_context(|| format!("parse Tesseract confidence on TSV row {}", index + 1))?;
        if confidence >= 0.0 {
            page.weighted_confidence += confidence * f64::from(confidence_characters);
            page.confidence_characters = page
                .confidence_characters
                .checked_add(confidence_characters)
                .context("OCR confidence character count overflowed")?;
        }
        page.lines
            .entry((block, paragraph, line))
            .or_default()
            .push(word);
    }
    if pages.is_empty() {
        pages.insert(1, PageBuilder::default());
    }
    let mut warnings = Vec::new();
    let pages = pages
        .into_iter()
        .map(|(page_number, page)| {
            let content = page
                .lines
                .into_values()
                .map(|words| words.join(" "))
                .collect::<Vec<_>>()
                .join("\n");
            let confidence = (page.confidence_characters > 0).then(|| {
                confidence_as_f32(page.weighted_confidence / f64::from(page.confidence_characters))
            });
            if content.is_empty() {
                warnings.push(format!("page {page_number} contained no recognized text"));
            }
            OcrPage {
                page_number,
                content,
                confidence,
                width: page.width,
                height: page.height,
            }
        })
        .collect();
    Ok(OcrResult {
        pages,
        language: language.to_owned(),
        engine_version: engine_version.to_owned(),
        warnings,
        peak_memory_bytes: None,
    })
}

#[allow(clippy::cast_possible_truncation)]
fn confidence_as_f32(confidence: f64) -> f32 {
    confidence as f32
}

fn parse_i32(value: &str, row_index: usize) -> Result<i32> {
    value
        .parse()
        .with_context(|| format!("parse Tesseract TSV row {}", row_index + 1))
}

#[cfg(all(test, unix))]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

    use uuid::Uuid;

    use super::{
        OcrNormalizationLimits, extract_ocr, is_supported_ocr_mime, normalize_ocr_input,
        verify_tesseract,
    };

    #[tokio::test]
    async fn extracts_typed_text_confidence_dimensions_and_version() {
        let root = std::env::temp_dir().join(format!("strife-ocr-adapter-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create OCR fixture directory");
        let binary = root.join("tesseract-fixture");
        fs::write(
            &binary,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "tesseract 5.5.0-fixture"
  exit 0
fi
printf 'level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n'
printf '1\t1\t0\t0\t0\t0\t0\t0\t640\t320\t-1\t\n'
printf '5\t1\t1\t1\t1\t1\t10\t10\t80\t24\t90.0\tKnown\n'
printf '5\t1\t1\t1\t1\t2\t100\t10\t60\t24\t60.0\ttext\n'
"#,
        )
        .expect("write fake Tesseract");
        let mut permissions = fs::metadata(&binary)
            .expect("binary metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions).expect("make fake Tesseract executable");
        let input = root.join("fixture.png");
        fs::write(&input, b"fixture").expect("write OCR input");

        let version = verify_tesseract(binary.to_str().expect("binary path"))
            .await
            .expect("verify Tesseract");
        assert_eq!(version, "tesseract 5.5.0-fixture");
        let result = extract_ocr(
            &input,
            binary.to_str().expect("binary path"),
            "eng",
            &version,
            Duration::from_secs(2),
            4096,
        )
        .await
        .expect("extract fixture OCR");
        assert_eq!(result.language, "eng");
        assert_eq!(result.engine_version, version);
        assert_eq!(result.pages.len(), 1);
        assert_eq!(result.pages[0].content, "Known text");
        assert_eq!(
            (result.pages[0].width, result.pages[0].height),
            (Some(640), Some(320))
        );
        let confidence = result.pages[0].confidence.expect("page confidence");
        assert!((confidence - 76.666_664).abs() < 0.01);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn normalizes_raster_families_and_enforces_page_and_pixel_limits() {
        if std::process::Command::new("magick")
            .arg("-version")
            .output()
            .is_err()
        {
            return;
        }
        let root = std::env::temp_dir().join(format!("strife-ocr-inputs-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create OCR inputs");
        let limits = OcrNormalizationLimits {
            raster_dpi: 200,
            max_pages: 3,
            max_pixels_per_page: 10_000,
            memory_limit_bytes: 64 * 1024 * 1024,
            process_timeout: Duration::from_secs(5),
        };
        for (name, mime) in [
            ("input.jpg", "image/jpeg"),
            ("input.png", "image/png"),
            ("input.webp", "image/webp"),
        ] {
            let path = root.join(name);
            assert!(
                std::process::Command::new("magick")
                    .args(["-size", "32x16", "xc:white"])
                    .arg(&path)
                    .status()
                    .expect("generate raster fixture")
                    .success()
            );
            let normalized = normalize_ocr_input(&path, mime, limits)
                .await
                .expect("normalize raster family");
            assert_eq!(normalized.pages.len(), 1);
            assert_eq!(
                (normalized.pages[0].width, normalized.pages[0].height),
                (32, 16)
            );
        }
        let tiff = root.join("multi.tiff");
        assert!(
            std::process::Command::new("magick")
                .args(["-size", "8x8", "xc:red", "xc:blue"])
                .arg(&tiff)
                .status()
                .expect("generate TIFF")
                .success()
        );
        assert_eq!(
            normalize_ocr_input(&tiff, "image/tiff", limits)
                .await
                .expect("normalize TIFF")
                .pages
                .len(),
            2
        );
        let page_error = normalize_ocr_input(
            &tiff,
            "image/tiff",
            OcrNormalizationLimits {
                max_pages: 1,
                ..limits
            },
        )
        .await
        .expect_err("TIFF must exceed page limit");
        assert!(format!("{page_error:#}").contains("page limit exceeded"));
        let pixel_error = normalize_ocr_input(
            &root.join("input.png"),
            "image/png",
            OcrNormalizationLimits {
                max_pixels_per_page: 10,
                ..limits
            },
        )
        .await
        .expect_err("PNG must exceed pixel limit");
        assert!(format!("{pixel_error:#}").contains("pixel limit exceeded"));
        assert!(is_supported_ocr_mime("application/pdf"));
        assert!(is_supported_ocr_mime("image/x-nikon-nef"));
        assert!(!is_supported_ocr_mime("image/gif"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn tesseract_timeout_is_bounded() {
        let root = std::env::temp_dir().join(format!("strife-ocr-timeout-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create timeout fixture directory");
        let binary = root.join("stalling-tesseract");
        fs::write(&binary, "#!/bin/sh\nsleep 5\n").expect("write stalling process");
        let mut permissions = fs::metadata(&binary)
            .expect("binary metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions).expect("make stalling process executable");
        let input = root.join("fixture.png");
        fs::write(&input, b"fixture").expect("write timeout input");
        let started = std::time::Instant::now();
        let error = extract_ocr(
            &input,
            binary.to_str().expect("binary path"),
            "eng",
            "fixture",
            Duration::from_millis(50),
            4096,
        )
        .await
        .expect_err("stalling OCR must time out");
        assert!(format!("{error:#}").contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(1));
        let _ = fs::remove_dir_all(root);
    }
}
