//! Metadata extractor and preview adapters for Strife.

use std::{path::Path, process::Command};

use anyhow::Result;

mod exif;
mod ffprobe;
mod process;

pub use exif::{ExifResult, extract_exif, extract_exif_with_limits};
pub use ffprobe::{
    FfprobeResult, StreamInfo, StreamType, extract_ffprobe, extract_ffprobe_with_limits,
};

/// Detects a file's MIME type from its bytes using the host's libmagic database.
///
/// Detection failures deliberately become `application/octet-stream` so an unknown file can
/// still progress through the metadata pipeline.
///
/// # Errors
///
/// This adapter currently converts detector failures into the generic MIME fallback.
pub fn detect_mime(path: &Path) -> Result<String> {
    if !path.is_file() {
        return Ok("application/octet-stream".to_owned());
    }
    let detected = Command::new("file")
        .arg("--brief")
        .arg("--mime-type")
        .arg("--")
        .arg(path)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|mime| mime.trim().to_owned())
        .filter(|mime| !mime.is_empty());

    if detected.as_deref() == Some("application/zip") && is_docx_package(path) {
        return Ok(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_owned(),
        );
    }
    Ok(detected.unwrap_or_else(|| "application/octet-stream".to_owned()))
}

fn is_docx_package(path: &Path) -> bool {
    Command::new("unzip")
        .args(["-Z1", "--"])
        .arg(path)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .is_some_and(|listing| {
            let mut has_content_types = false;
            let mut has_document = false;
            for entry in listing.lines() {
                has_content_types |= entry == "[Content_Types].xml";
                has_document |= entry == "word/document.xml";
            }
            has_content_types && has_document
        })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, process::Command};

    use super::detect_mime;
    use uuid::Uuid;

    struct FixtureDir(std::path::PathBuf);

    impl FixtureDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("strife-mime-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("create fixture directory");
            Self(path)
        }

        fn write(&self, name: &str, bytes: &[u8]) -> std::path::PathBuf {
            let path = self.0.join(name);
            fs::write(&path, bytes).expect("write fixture");
            path
        }
    }

    impl Drop for FixtureDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn assert_mime(path: &Path, expected: &str) {
        assert_eq!(detect_mime(path).expect("detect MIME"), expected);
    }

    #[test]
    fn detects_supported_formats_from_content() {
        let fixtures = FixtureDir::new();
        let jpeg = fixtures.write(
            "image.bin",
            b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00\xff\xd9",
        );
        let png = fixtures.write(
            "image.data",
            b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde",
        );
        let pdf = fixtures.write("document.bin", b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\n%%EOF\n");
        let mp4 = fixtures.write(
            "video.bin",
            b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2",
        );
        let mp3 = fixtures.write(
            "audio.bin",
            b"ID3\x04\x00\x00\x00\x00\x00\x00\xff\xfb\x90\x64",
        );

        assert_mime(&jpeg, "image/jpeg");
        assert_mime(&png, "image/png");
        assert_mime(&pdf, "application/pdf");
        assert_mime(&mp4, "video/mp4");
        assert_mime(&mp3, "audio/mpeg");
    }

    #[test]
    fn detects_docx_and_extensionless_content() {
        let fixtures = FixtureDir::new();
        let content_types = fixtures.write(
            "[Content_Types].xml",
            br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        );
        let word_dir = fixtures.0.join("word");
        fs::create_dir(&word_dir).expect("create word directory");
        fs::write(word_dir.join("document.xml"), "<w:document/>").expect("write document part");
        let docx = fixtures.0.join("document.docx");
        let status = Command::new("zip")
            .current_dir(&fixtures.0)
            .args(["-q", docx.to_str().expect("UTF-8 path")])
            .arg(content_types.file_name().expect("content types name"))
            .arg("word/document.xml")
            .status()
            .expect("run zip");
        assert!(status.success());
        assert_mime(
            &docx,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        );

        let extensionless = fixtures.write("no-extension", b"%PDF-1.7\n%%EOF\n");
        assert_mime(&extensionless, "application/pdf");
    }

    #[test]
    fn missing_path_uses_binary_fallback() {
        assert_mime(
            Path::new("/definitely/not/a/strife/file"),
            "application/octet-stream",
        );
    }
}
