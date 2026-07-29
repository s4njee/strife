//! Opaque-key file storage abstractions for Strife.

use std::{
    path::{Path, PathBuf},
    pin::Pin,
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom},
};
use uuid::Uuid;

/// A heap-owned asynchronous reader returned by storage backends.
pub type StorageReader = Pin<Box<dyn AsyncRead + Send>>;

/// Managed storage namespaces with deliberately separate lifecycles.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StorageNamespace {
    Staging,
    Originals,
    Artifacts,
}

impl StorageNamespace {
    const fn directory(self) -> &'static str {
        match self {
            Self::Staging => "staging",
            Self::Originals => "originals",
            Self::Artifacts => "artifacts",
        }
    }
}

/// An opaque UUID storage key scoped to a managed namespace.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct StorageKey {
    namespace: StorageNamespace,
    id: Uuid,
}

impl StorageKey {
    #[must_use]
    pub const fn new(namespace: StorageNamespace, id: Uuid) -> Self {
        Self { namespace, id }
    }

    #[must_use]
    pub const fn staging(id: Uuid) -> Self {
        Self::new(StorageNamespace::Staging, id)
    }

    #[must_use]
    pub const fn original(id: Uuid) -> Self {
        Self::new(StorageNamespace::Originals, id)
    }

    #[must_use]
    pub const fn artifact(id: Uuid) -> Self {
        Self::new(StorageNamespace::Artifacts, id)
    }

    #[must_use]
    pub const fn id(self) -> Uuid {
        self.id
    }

    #[must_use]
    pub const fn namespace(self) -> StorageNamespace {
        self.namespace
    }
}

/// Capacity statistics for the storage volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiskUsage {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

/// Backend contract for managed originals, staging objects, and artifacts.
#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn put_stream(&self, key: StorageKey, reader: StorageReader) -> Result<()>;
    async fn write_range(&self, key: StorageKey, offset: u64, reader: StorageReader)
    -> Result<u64>;
    async fn move_object(&self, source: StorageKey, destination: StorageKey) -> Result<()>;
    async fn detect_mime(&self, key: StorageKey) -> Result<String>;
    async fn get_stream(&self, key: StorageKey) -> Result<StorageReader>;
    async fn get_range(&self, key: StorageKey, offset: u64, length: u64) -> Result<StorageReader>;
    async fn delete(&self, key: StorageKey) -> Result<()>;
    async fn exists(&self, key: StorageKey) -> Result<bool>;
    async fn disk_usage(&self) -> Result<DiskUsage>;
}

/// Local-filesystem storage rooted at one configured directory.
#[derive(Clone, Debug)]
pub struct LocalFsBackend {
    root: PathBuf,
}

impl LocalFsBackend {
    /// Creates the three managed namespaces below `root`.
    ///
    /// # Errors
    ///
    /// Returns an error when a namespace cannot be created.
    pub async fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let backend = Self { root: root.into() };
        for namespace in [
            StorageNamespace::Staging,
            StorageNamespace::Originals,
            StorageNamespace::Artifacts,
        ] {
            fs::create_dir_all(backend.root.join(namespace.directory()))
                .await
                .with_context(|| format!("create {} namespace", namespace.directory()))?;
        }
        Ok(backend)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for(&self, key: StorageKey) -> PathBuf {
        self.root
            .join(key.namespace.directory())
            .join(key.id.simple().to_string())
    }
}

#[async_trait]
impl StorageBackend for LocalFsBackend {
    async fn put_stream(&self, key: StorageKey, mut reader: StorageReader) -> Result<()> {
        let destination = self.path_for(key);
        let temporary = destination.with_file_name(format!(".{}.tmp", Uuid::new_v4().simple()));
        let result = async {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .await
                .context("create temporary storage object")?;
            tokio::io::copy(&mut reader, &mut file)
                .await
                .context("stream storage object")?;
            file.flush().await.context("flush storage object")?;
            file.sync_all().await.context("sync storage object")?;
            drop(file);
            fs::rename(&temporary, &destination)
                .await
                .context("atomically publish storage object")
        }
        .await;

        if result.is_err() {
            let _ = fs::remove_file(&temporary).await;
        }
        result
    }

    async fn write_range(
        &self,
        key: StorageKey,
        offset: u64,
        mut reader: StorageReader,
    ) -> Result<u64> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.path_for(key))
            .await
            .context("open staging object for ranged write")?;
        file.seek(SeekFrom::Start(offset)).await?;
        let written = tokio::io::copy(&mut reader, &mut file)
            .await
            .context("stream staging object range")?;
        file.flush().await.context("flush staging object range")?;
        Ok(written)
    }

    async fn move_object(&self, source: StorageKey, destination: StorageKey) -> Result<()> {
        fs::rename(self.path_for(source), self.path_for(destination))
            .await
            .context("move managed storage object")
    }

    async fn detect_mime(&self, key: StorageKey) -> Result<String> {
        let output = tokio::process::Command::new("file")
            .arg("--brief")
            .arg("--mime-type")
            .arg(self.path_for(key))
            .output()
            .await
            .context("run content MIME detection")?;
        if !output.status.success() {
            bail!("content MIME detection failed");
        }
        let mime = String::from_utf8(output.stdout)
            .context("MIME detector returned invalid text")?
            .trim()
            .to_owned();
        if mime.is_empty() {
            bail!("MIME detector returned an empty result");
        }
        Ok(mime)
    }

    async fn get_stream(&self, key: StorageKey) -> Result<StorageReader> {
        let file = File::open(self.path_for(key))
            .await
            .context("open storage object")?;
        Ok(Box::pin(file))
    }

    async fn get_range(&self, key: StorageKey, offset: u64, length: u64) -> Result<StorageReader> {
        let mut file = File::open(self.path_for(key))
            .await
            .context("open storage object range")?;
        let size = file.metadata().await?.len();
        if offset > size {
            bail!("range offset exceeds storage object size");
        }
        file.seek(SeekFrom::Start(offset)).await?;
        Ok(Box::pin(file.take(length.min(size - offset))))
    }

    async fn delete(&self, key: StorageKey) -> Result<()> {
        match fs::remove_file(self.path_for(key)).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("delete storage object"),
        }
    }

    async fn exists(&self, key: StorageKey) -> Result<bool> {
        match fs::metadata(self.path_for(key)).await {
            Ok(metadata) => Ok(metadata.is_file()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error).context("inspect storage object"),
        }
    }

    async fn disk_usage(&self) -> Result<DiskUsage> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let total_bytes = fs2::total_space(&root).context("read total storage capacity")?;
            let available_bytes =
                fs2::available_space(&root).context("read available storage capacity")?;
            Ok(DiskUsage {
                total_bytes,
                used_bytes: total_bytes.saturating_sub(available_bytes),
                available_bytes,
            })
        })
        .await
        .context("join disk usage task")?
    }
}
