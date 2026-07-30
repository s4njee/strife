use crate::process::run_bounded;
use anyhow::{Context, Result};
use std::{path::Path, time::Duration};
use tokio::process::Command;
use uuid::Uuid;

/// Converts a DOC/DOCX file to PDF in an isolated headless `LibreOffice` profile.
///
/// # Errors
/// Returns an error when conversion times out, fails, or produces no PDF.
pub async fn convert_office_to_pdf(source: &Path, dest: &Path) -> Result<u64> {
    let root = std::env::temp_dir().join(format!("strife-office-{}", Uuid::new_v4()));
    let out = root.join("out");
    let profile = root.join("profile");
    tokio::fs::create_dir_all(&out).await?;
    let mut command = Command::new("soffice");
    command
        .arg("--headless")
        .arg(format!(
            "-env:UserInstallation=file://{}",
            profile.display()
        ))
        .args(["--convert-to", "pdf", "--outdir"])
        .arg(&out)
        .arg(source);
    let result = run_bounded(
        command,
        "LibreOffice conversion",
        Duration::from_secs(120),
        1024 * 1024,
    )
    .await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_dir_all(&root).await;
        return Err(error);
    }
    let generated = out
        .join(source.file_stem().context("office file has no stem")?)
        .with_extension("pdf");
    tokio::fs::rename(&generated, dest)
        .await
        .context("publish converted PDF")?;
    let size = tokio::fs::metadata(dest).await?.len();
    let _ = tokio::fs::remove_dir_all(root).await;
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::convert_office_to_pdf;
    use std::{fs, process::Command};
    use uuid::Uuid;
    #[tokio::test]
    async fn converts_representative_docx() {
        if Command::new("soffice").arg("--version").output().is_err() {
            return;
        }
        let root = std::env::temp_dir().join(format!("strife-docx-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let html = root.join("report.html");
        fs::write(
            &html,
            "<h1>Strife</h1><table><tr><td>Preview</td></tr></table>",
        )
        .unwrap();
        let generated = Command::new("soffice")
            .arg("--headless")
            .arg(format!(
                "-env:UserInstallation=file://{}",
                root.join("generator-profile").display()
            ))
            .args(["--convert-to", "docx", "--outdir"])
            .arg(&root)
            .arg(&html)
            .status()
            .unwrap();
        if !generated.success() {
            eprintln!("LibreOffice DOCX export filter unavailable; skipping");
            let _ = fs::remove_dir_all(root);
            return;
        }
        let pdf = root.join("preview.pdf");
        let size = convert_office_to_pdf(&root.join("report.docx"), &pdf)
            .await
            .unwrap();
        assert!(size > 100);
        assert!(fs::read(&pdf).unwrap().starts_with(b"%PDF"));
        let _ = fs::remove_dir_all(root);
    }
}
