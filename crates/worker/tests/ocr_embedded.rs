use std::{sync::Arc, time::Duration as StdDuration};

use axum::{Router, body::Bytes, http::HeaderMap, routing::put};
use chrono::Duration;
use sqlx::{PgPool, postgres::PgPoolOptions};
use strife_db::{
    DocumentTextSource, DocumentTextStatus, JobState, JobType, MIGRATOR, ROOT_NODE_ID, claim_job,
    complete_job, create_file_object, enqueue_job, fail_job, finalize_file_object,
    get_document_text, get_job, list_document_text_pages,
};
use strife_storage::{LocalFsBackend, StorageBackend, StorageKey};
use strife_worker::{JobHandler, OcrHandler, OcrSettings};
use uuid::Uuid;

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&database_url)
        .await
        .expect("connect to integration-test PostgreSQL");
    MIGRATOR.run(&pool).await.expect("apply migrations");
    Some(pool)
}

async fn clear_stale_active_ocr_jobs(pool: &PgPool) {
    sqlx::query("DELETE FROM jobs WHERE job_type = 'ocr' AND state IN ('pending', 'leased')")
        .execute(pool)
        .await
        .expect("clear stale OCR integration-test jobs");
}

#[cfg(unix)]
fn fake_tesseract(root: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let binary = root.join("tesseract-fixture");
    std::fs::write(
        &binary,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo "tesseract 5.5-worker-fixture"; exit 0; fi
printf 'level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n'
printf '1\t1\t0\t0\t0\t0\t0\t0\t320\t160\t-1\t\n'
printf '5\t1\t1\t1\t1\t1\t10\t10\t80\t24\t92.0\tFixture\n'
printf '5\t1\t1\t1\t1\t2\t100\t10\t60\t24\t88.0\timage\n'
"#,
    )
    .expect("write fake Tesseract");
    let mut permissions = std::fs::metadata(&binary)
        .expect("binary metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&binary, permissions).expect("make fake Tesseract executable");
    binary
}

async fn embedded_text(headers: HeaderMap, body: Bytes) -> &'static str {
    assert_eq!(
        headers
            .get("x-tika-pdfocrstrategy")
            .and_then(|value| value.to_str().ok()),
        Some("no_ocr")
    );
    assert!(body.starts_with(b"%PDF"));
    "This PDF already contains a useful embedded text layer."
}

async fn no_embedded_text(headers: HeaderMap, body: Bytes) -> &'static str {
    assert_eq!(
        headers
            .get("x-tika-pdfocrstrategy")
            .and_then(|value| value.to_str().ok()),
        Some("no_ocr")
    );
    assert!(body.starts_with(b"%PDF"));
    ""
}

fn image_only_pdf(page_count: usize) -> Vec<u8> {
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0_usize];
    let page_ids = (0..page_count).map(|index| index + 3).collect::<Vec<_>>();
    let kids = page_ids
        .iter()
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {page_count} >>").into_bytes(),
    ];
    for _ in 0..page_count {
        objects.push(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 72 72] >>".to_vec());
    }
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.into_iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn text_pdf_is_persisted_and_the_ocr_job_is_skipped() {
    let Some(pool) = test_pool().await else {
        eprintln!("DATABASE_URL is unset; skipping PostgreSQL integration test");
        return;
    };
    let mut ocr_lock = pool.begin().await.expect("begin OCR test lock");
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(0x4f43_5200_i64)
        .execute(&mut *ocr_lock)
        .await
        .expect("acquire OCR test lock");
    clear_stale_active_ocr_jobs(&pool).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock Tika");
    let address = listener.local_addr().expect("mock Tika address");
    let server = tokio::spawn(
        axum::serve(listener, Router::new().route("/tika", put(embedded_text))).into_future(),
    );
    let storage_root = std::env::temp_dir().join(format!("strife-ocr-{}", Uuid::new_v4()));
    let storage = Arc::new(
        LocalFsBackend::new(&storage_root)
            .await
            .expect("create storage"),
    );
    let storage_id = Uuid::new_v4();
    let pdf = b"%PDF-1.7\ntext-layer\n%%EOF";
    storage
        .put_stream(
            StorageKey::original(storage_id),
            Box::pin(std::io::Cursor::new(pdf.to_vec())),
        )
        .await
        .expect("store PDF");
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("embedded-{node_id}.pdf"))
        .execute(&pool)
        .await
        .expect("create PDF node");
    let file = create_file_object(
        &pool,
        storage_id,
        i64::try_from(pdf.len()).expect("PDF size"),
        Some("application/pdf"),
        None,
    )
    .await
    .expect("create file object");
    finalize_file_object(&pool, file.id, node_id)
        .await
        .expect("finalize file object");
    enqueue_job(&pool, JobType::Ocr, node_id, -10)
        .await
        .expect("enqueue OCR")
        .expect("new OCR job");
    let job = claim_job(&pool, JobType::Ocr, "ocr-test", Duration::minutes(1))
        .await
        .expect("claim OCR")
        .expect("leased OCR");

    OcrHandler::new(
        pool.clone(),
        storage.clone(),
        format!("http://{address}"),
        10,
    )
    .handle(&job)
    .await
    .expect("detect embedded PDF text");

    let record = get_document_text(&pool, node_id)
        .await
        .expect("load document text")
        .expect("stored document text");
    assert_eq!(record.source, DocumentTextSource::Embedded);
    assert_eq!(record.status, DocumentTextStatus::Completed);
    assert_eq!(record.page_count, Some(1));
    assert_eq!(record.warnings.len(), 1);
    let pages = list_document_text_pages(&pool, node_id)
        .await
        .expect("load embedded page");
    assert_eq!(pages.len(), 1);
    assert!(pages[0].content.contains("useful embedded text"));
    assert_eq!(
        get_job(&pool, job.id)
            .await
            .expect("load OCR job")
            .expect("OCR job row")
            .state,
        JobState::Skipped
    );

    sqlx::query("DELETE FROM file_objects WHERE node_id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("clean up PDF object");
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("clean up PDF node");
    server.abort();
    let _ = tokio::fs::remove_dir_all(storage_root).await;
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn raster_image_completes_idempotently_and_corrupt_image_records_failure() {
    let Some(pool) = test_pool().await else {
        return;
    };
    if std::process::Command::new("magick")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("ImageMagick is unavailable; skipping OCR normalization integration test");
        return;
    }
    let mut ocr_lock = pool.begin().await.expect("begin OCR test lock");
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(0x4f43_5200_i64)
        .execute(&mut *ocr_lock)
        .await
        .expect("acquire OCR test lock");
    clear_stale_active_ocr_jobs(&pool).await;
    let storage_root = std::env::temp_dir().join(format!("strife-ocr-raster-{}", Uuid::new_v4()));
    let fixture_root = std::env::temp_dir().join(format!("strife-ocr-tools-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&fixture_root).expect("create OCR tools directory");
    let tesseract = fake_tesseract(&fixture_root);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind scanned-PDF Tika fixture");
    let address = listener.local_addr().expect("scanned-PDF Tika address");
    let tika_server = tokio::spawn(
        axum::serve(
            listener,
            Router::new().route("/tika", put(no_embedded_text)),
        )
        .into_future(),
    );
    let image = fixture_root.join("known.png");
    assert!(
        std::process::Command::new("magick")
            .args(["-size", "320x160", "xc:white"])
            .arg(&image)
            .status()
            .expect("generate PNG")
            .success()
    );
    let storage = Arc::new(LocalFsBackend::new(&storage_root).await.expect("storage"));
    let storage_id = Uuid::new_v4();
    let bytes = tokio::fs::read(&image).await.expect("read PNG");
    storage
        .put_stream(
            StorageKey::original(storage_id),
            Box::pin(std::io::Cursor::new(bytes.clone())),
        )
        .await
        .expect("store PNG");
    let node_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(node_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("ocr-raster-{node_id}.png"))
        .execute(&pool)
        .await
        .expect("create image node");
    let file = create_file_object(
        &pool,
        storage_id,
        i64::try_from(bytes.len()).expect("image size"),
        Some("image/png"),
        None,
    )
    .await
    .expect("create image object");
    finalize_file_object(&pool, file.id, node_id)
        .await
        .expect("finalize image object");
    enqueue_job(&pool, JobType::Ocr, node_id, i32::MAX)
        .await
        .expect("enqueue image OCR");
    let job = claim_job(&pool, JobType::Ocr, "ocr-raster", Duration::minutes(1))
        .await
        .expect("claim image OCR")
        .expect("leased image OCR");
    let settings = OcrSettings {
        tesseract_binary: tesseract.to_string_lossy().into_owned(),
        engine_version: "tesseract 5.5-worker-fixture".to_owned(),
        file_timeout: StdDuration::from_secs(10),
        ..OcrSettings::default()
    };
    let handler = OcrHandler::new(
        pool.clone(),
        storage.clone(),
        format!("http://{address}"),
        20,
    )
    .with_settings(settings);
    handler.handle(&job).await.expect("OCR image");
    complete_job(&pool, job.id)
        .await
        .expect("complete image OCR job")
        .expect("leased image OCR completed");
    let document = get_document_text(&pool, node_id)
        .await
        .expect("load OCR document")
        .expect("OCR document");
    assert_eq!(document.status, DocumentTextStatus::Completed);
    assert_eq!(document.source, DocumentTextSource::Ocr);
    assert!(document.mean_confidence.is_some());
    assert!(document.duration_ms.is_some());
    let pages = list_document_text_pages(&pool, node_id)
        .await
        .expect("load OCR pages");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].content, "Fixture image");

    enqueue_job(&pool, JobType::Ocr, node_id, i32::MAX)
        .await
        .expect("enqueue image OCR rerun");
    let rerun = claim_job(
        &pool,
        JobType::Ocr,
        "ocr-raster-rerun",
        Duration::minutes(1),
    )
    .await
    .expect("claim image OCR rerun")
    .expect("leased image OCR rerun");
    handler.handle(&rerun).await.expect("rerun image OCR");
    complete_job(&pool, rerun.id)
        .await
        .expect("complete image OCR rerun")
        .expect("leased image OCR rerun completed");
    let rerun_pages = list_document_text_pages(&pool, node_id)
        .await
        .expect("load rerun OCR pages");
    assert_eq!(rerun_pages.len(), 1, "rerun must replace existing pages");
    assert_eq!(rerun_pages[0].content, "Fixture image");

    let pdf_storage_id = Uuid::new_v4();
    let pdf = image_only_pdf(2);
    storage
        .put_stream(
            StorageKey::original(pdf_storage_id),
            Box::pin(std::io::Cursor::new(pdf.clone())),
        )
        .await
        .expect("store scanned PDF");
    let pdf_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(pdf_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("ocr-scanned-{pdf_id}.pdf"))
        .execute(&pool)
        .await
        .expect("create scanned PDF node");
    let pdf_file = create_file_object(
        &pool,
        pdf_storage_id,
        i64::try_from(pdf.len()).expect("PDF size"),
        Some("application/pdf"),
        None,
    )
    .await
    .expect("create scanned PDF object");
    finalize_file_object(&pool, pdf_file.id, pdf_id)
        .await
        .expect("finalize scanned PDF object");
    enqueue_job(&pool, JobType::Ocr, pdf_id, i32::MAX)
        .await
        .expect("enqueue scanned PDF OCR");
    let pdf_job = claim_job(&pool, JobType::Ocr, "ocr-scanned", Duration::minutes(1))
        .await
        .expect("claim scanned PDF OCR")
        .expect("leased scanned PDF OCR");
    handler.handle(&pdf_job).await.expect("OCR scanned PDF");
    complete_job(&pool, pdf_job.id)
        .await
        .expect("complete scanned PDF OCR")
        .expect("leased scanned PDF OCR completed");
    let pdf_document = get_document_text(&pool, pdf_id)
        .await
        .expect("load scanned PDF OCR document")
        .expect("scanned PDF OCR document");
    assert_eq!(pdf_document.source, DocumentTextSource::Ocr);
    assert_eq!(pdf_document.page_count, Some(2));
    let pdf_pages = list_document_text_pages(&pool, pdf_id)
        .await
        .expect("load scanned PDF OCR pages");
    assert_eq!(
        pdf_pages
            .iter()
            .map(|page| page.page_number)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );

    let corrupt_storage_id = Uuid::new_v4();
    let corrupt = bytes[..64.min(bytes.len())].to_vec();
    storage
        .put_stream(
            StorageKey::original(corrupt_storage_id),
            Box::pin(std::io::Cursor::new(corrupt.clone())),
        )
        .await
        .expect("store corrupt PNG");
    let corrupt_id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes (id, parent_id, name, kind) VALUES ($1, $2, $3, 'file')")
        .bind(corrupt_id)
        .bind(ROOT_NODE_ID)
        .bind(format!("ocr-corrupt-{corrupt_id}.png"))
        .execute(&pool)
        .await
        .expect("create corrupt node");
    let corrupt_file = create_file_object(
        &pool,
        corrupt_storage_id,
        i64::try_from(corrupt.len()).expect("corrupt size"),
        Some("image/png"),
        None,
    )
    .await
    .expect("create corrupt object");
    finalize_file_object(&pool, corrupt_file.id, corrupt_id)
        .await
        .expect("finalize corrupt object");
    enqueue_job(&pool, JobType::Ocr, corrupt_id, i32::MAX)
        .await
        .expect("enqueue corrupt OCR");
    let corrupt_job = claim_job(&pool, JobType::Ocr, "ocr-corrupt", Duration::minutes(1))
        .await
        .expect("claim corrupt OCR")
        .expect("leased corrupt OCR");
    let error = handler
        .handle(&corrupt_job)
        .await
        .expect_err("corrupt image must fail");
    let failed = fail_job(&pool, corrupt_job.id, &format!("{error:#}"))
        .await
        .expect("record corrupt job failure")
        .expect("failed corrupt job attempt");
    assert!(failed.last_error.is_some());
    let failure = get_document_text(&pool, corrupt_id)
        .await
        .expect("load corrupt OCR state")
        .expect("corrupt OCR state");
    assert_eq!(failure.status, DocumentTextStatus::Failed);
    assert!(!failure.warnings.is_empty());

    for id in [node_id, pdf_id, corrupt_id] {
        sqlx::query("DELETE FROM file_objects WHERE node_id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .expect("delete OCR object");
        sqlx::query("DELETE FROM nodes WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .expect("delete OCR node");
    }
    let _ = tokio::fs::remove_dir_all(storage_root).await;
    let _ = tokio::fs::remove_dir_all(fixture_root).await;
    tika_server.abort();
}
