use std::{
    io::{ErrorKind, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};

const COPY_BUFFER_BYTES: usize = 1024 * 1024;
const SNIFF_BYTES: usize = 512;

#[derive(Debug, Clone)]
pub struct DataLayout {
    pub root: PathBuf,
    pub media: PathBuf,
    pub derived: PathBuf,
    pub projects: PathBuf,
    pub exports: PathBuf,
    pub temporary: PathBuf,
}

impl DataLayout {
    pub async fn initialize(root: impl Into<PathBuf>) -> Result<Self> {
        let requested_root = root.into();
        fs::create_dir_all(&requested_root).await?;
        let root = fs::canonicalize(&requested_root)
            .await
            .with_context(|| format!("canonicalize data root {}", requested_root.display()))?;
        let layout = Self {
            media: root.join("media/sha256"),
            derived: root.join("derived/sha256"),
            projects: root.join("projects"),
            exports: root.join("exports"),
            temporary: root.join("tmp"),
            root,
        };
        for directory in [
            &layout.root,
            &layout.media,
            &layout.derived,
            &layout.projects,
            &layout.exports,
            &layout.temporary,
        ] {
            fs::create_dir_all(directory)
                .await
                .with_context(|| format!("create data directory {}", directory.display()))?;
            // The daemon data tree is private even when it predates this
            // process. Do not inherit an accidentally permissive umask.
            set_private_directory(directory).await?;
            ensure_beneath(&layout.root, directory).await?;
        }
        Ok(layout)
    }

    /// Persist bytes under their SHA-256 digest. The two-level fanout keeps large
    /// libraries from placing every asset in one directory. Existing content is
    /// never overwritten.
    pub async fn put_media(&self, bytes: &[u8]) -> Result<StoredContent> {
        put_content(&self.media, bytes).await
    }

    /// Atomically install a daemon-produced export. The default no-overwrite
    /// path uses a hard-link create so another process can never be replaced
    /// between the caller's preflight and installation.
    pub async fn install_export_bytes(
        &self,
        file_name: &str,
        bytes: &[u8],
        allow_overwrite: bool,
    ) -> Result<PathBuf> {
        let destination = self.exports.join(file_name);
        ensure_beneath(&self.root, &self.exports).await?;
        let temporary = self
            .temporary
            .join(format!(".export-{}.tmp", uuid::Uuid::new_v4()));
        let mut file = create_private_file(&temporary).await?;
        file.write_all(bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        if allow_overwrite {
            match fs::rename(&temporary, &destination).await {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    fs::remove_file(&destination).await?;
                    fs::rename(&temporary, &destination).await?;
                }
                Err(error) => {
                    let _ = fs::remove_file(&temporary).await;
                    return Err(error.into());
                }
            }
        } else if !install_temporary(&temporary, &destination).await? {
            bail!("export destination already exists");
        }
        Ok(destination)
    }

    /// Atomically move a completed daemon-side artifact from the private temp
    /// directory into exports. Large project packages use this path so their
    /// media is never accumulated in memory.
    pub async fn install_export_file(
        &self,
        temporary: &Path,
        file_name: &str,
        allow_overwrite: bool,
    ) -> Result<PathBuf> {
        let destination = self.exports.join(file_name);
        let metadata = fs::symlink_metadata(temporary).await?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            bail!("temporary export artifact is not a regular file");
        }
        let parent = temporary
            .parent()
            .context("temporary export artifact has no parent")?;
        if fs::canonicalize(parent).await? != self.temporary {
            bail!("temporary export artifact is outside the daemon temp directory");
        }
        ensure_beneath(&self.root, &self.temporary).await?;
        ensure_beneath(&self.root, &self.exports).await?;
        if allow_overwrite {
            match fs::rename(temporary, &destination).await {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    fs::remove_file(&destination).await?;
                    fs::rename(temporary, &destination).await?;
                }
                Err(error) => return Err(error.into()),
            }
        } else if !install_temporary(temporary, &destination).await? {
            bail!("export destination already exists");
        }
        Ok(destination)
    }

    /// Stream a host file into the immutable managed media library. The source
    /// must already have passed the daemon's authorized-root policy.
    pub async fn put_media_file(&self, source: &Path, maximum_bytes: u64) -> Result<StoredContent> {
        let mut source = open_read_no_follow(source).await?;
        let hashed = hash_open_file(&mut source, maximum_bytes).await?;
        Ok(self
            .put_hashed_media_file(&mut source, &hashed, maximum_bytes)
            .await?
            .content)
    }

    pub async fn put_hashed_media_file(
        &self,
        source: &mut File,
        hashed: &HashedSource,
        maximum_bytes: u64,
    ) -> Result<InstalledContent> {
        put_file_content(&self.media, source, hashed, maximum_bytes).await
    }

    /// Resolve a managed digest without exposing arbitrary filesystem lookup.
    /// Missing content is reported as `None`; an escaping symlink or malformed
    /// digest is an integrity error.
    pub async fn media_content(&self, digest: &str) -> Result<Option<StoredContent>> {
        validate_digest(digest)?;
        let path = digest_path(&self.media, digest);
        let metadata = match fs::symlink_metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        ensure_beneath(&self.media, &path).await?;
        verify_existing_content(&path, digest, metadata.len()).await?;
        Ok(Some(StoredContent {
            sha256: digest.to_owned(),
            path,
            size: metadata.len(),
        }))
    }

    pub async fn remove_media_if_matches(&self, digest: &str) -> Result<bool> {
        let Some(content) = self.media_content(digest).await? else {
            return Ok(false);
        };
        fs::remove_file(content.path).await?;
        Ok(true)
    }

    /// Enumerate only structurally valid content-addressed media. Unexpected
    /// symlinks or non-regular entries abort the scan rather than being followed
    /// or deleted by maintenance code.
    pub async fn media_inventory(&self) -> Result<Vec<MediaInventoryEntry>> {
        ensure_beneath(&self.root, &self.media).await?;
        let mut inventory = Vec::new();
        let mut fanout = fs::read_dir(&self.media).await?;
        while let Some(directory) = fanout.next_entry().await? {
            let directory_name = directory.file_name().to_string_lossy().into_owned();
            let metadata = fs::symlink_metadata(directory.path()).await?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                bail!("unexpected non-directory entry in managed media store");
            }
            if directory_name.len() != 2
                || !directory_name
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                // Never treat unknown directories as collectible content.
                continue;
            }
            ensure_beneath(&self.media, &directory.path()).await?;
            let mut entries = fs::read_dir(directory.path()).await?;
            while let Some(entry) = entries.next_entry().await? {
                let file_name = entry.file_name().to_string_lossy().into_owned();
                // A crash may leave a hidden temporary file. It is not a valid
                // digest and is deliberately outside asset GC's authority.
                if file_name.starts_with('.') {
                    continue;
                }
                let digest = format!("{directory_name}{file_name}");
                if digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    continue;
                }
                let metadata = fs::symlink_metadata(entry.path()).await?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    bail!("unexpected non-regular entry in managed media store");
                }
                ensure_beneath(&self.media, &entry.path()).await?;
                inventory.push(MediaInventoryEntry {
                    sha256: digest,
                    path: entry.path(),
                    size: metadata.len(),
                    modified_at: metadata.modified()?,
                });
            }
        }
        inventory.sort_by(|left, right| left.sha256.cmp(&right.sha256));
        Ok(inventory)
    }

    pub async fn put_derived(&self, bytes: &[u8]) -> Result<StoredContent> {
        put_content(&self.derived, bytes).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredContent {
    pub sha256: String,
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInventoryEntry {
    pub sha256: String,
    pub path: PathBuf,
    pub size: u64,
    pub modified_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedSource {
    pub sha256: String,
    pub size: u64,
    pub prefix: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledContent {
    pub content: StoredContent,
    pub created: bool,
}

async fn put_content(root: &Path, bytes: &[u8]) -> Result<StoredContent> {
    let digest = hex::encode(Sha256::digest(bytes));
    let directory = root.join(&digest[..2]);
    let destination = digest_path(root, &digest);
    fs::create_dir_all(&directory).await?;
    ensure_beneath(root, &directory).await?;

    match fs::symlink_metadata(&destination).await {
        Ok(_) => false,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let temporary =
                directory.join(format!(".{}.{}.tmp", &digest[2..], uuid::Uuid::new_v4()));
            let mut output = create_private_file(&temporary).await?;
            output.write_all(bytes).await?;
            output.flush().await?;
            output.sync_all().await?;
            drop(output);
            install_temporary(&temporary, &destination).await?
        }
        Err(error) => return Err(error.into()),
    };
    verify_existing_content(&destination, &digest, bytes.len() as u64).await?;

    Ok(StoredContent {
        sha256: digest,
        path: destination,
        size: bytes.len() as u64,
    })
}

async fn put_file_content(
    root: &Path,
    input: &mut File,
    expected: &HashedSource,
    maximum_bytes: u64,
) -> Result<InstalledContent> {
    input.seek(SeekFrom::Start(0)).await?;
    let temporary = root.join(format!(".incoming-{}.tmp", uuid::Uuid::new_v4()));
    let mut output = create_private_file(&temporary).await?;
    let streamed = async {
        let mut hasher = Sha256::new();
        let mut total = 0_u64;
        let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
        loop {
            let read = input.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            total = total
                .checked_add(read as u64)
                .context("import source size overflow")?;
            if total > maximum_bytes {
                bail!("import source grew beyond the {maximum_bytes} byte limit");
            }
            hasher.update(&buffer[..read]);
            output.write_all(&buffer[..read]).await?;
        }
        output.flush().await?;
        output.sync_all().await?;
        if total != expected.size {
            bail!("import source changed size while it was being copied");
        }
        let actual_digest = hex::encode(hasher.finalize());
        if actual_digest != expected.sha256 {
            bail!("import source changed content while it was being copied");
        }
        Ok::<_, anyhow::Error>(total)
    }
    .await;
    drop(output);

    let size = match streamed {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_file(&temporary).await;
            return Err(error);
        }
    };
    let digest = &expected.sha256;
    let directory = root.join(&digest[..2]);
    let destination = digest_path(root, digest);
    let install = async {
        fs::create_dir_all(&directory).await?;
        set_private_directory(&directory).await?;
        ensure_beneath(root, &directory).await?;
        install_temporary(&temporary, &destination).await
    }
    .await;
    let created = install?;
    verify_existing_content(&destination, digest, size).await?;

    Ok(InstalledContent {
        content: StoredContent {
            sha256: digest.clone(),
            path: destination,
            size,
        },
        created,
    })
}

pub async fn open_read_no_follow(path: &Path) -> Result<File> {
    let path = path.to_owned();
    let file = tokio::task::spawn_blocking(move || {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_FLAG_OPEN_REPARSE_POINT prevents transparent traversal of a
            // final-component symlink/junction.
            options.custom_flags(0x0020_0000);
        }
        options.open(path)
    })
    .await
    .context("join no-follow file open")??;
    Ok(File::from_std(file))
}

pub async fn hash_open_file(input: &mut File, maximum_bytes: u64) -> Result<HashedSource> {
    input.seek(SeekFrom::Start(0)).await?;
    let metadata = input.metadata().await?;
    if !metadata.is_file() {
        bail!("import source is not a regular file");
    }
    if metadata.len() > maximum_bytes {
        bail!(
            "import source is {} bytes, exceeding the {} byte limit",
            metadata.len(),
            maximum_bytes
        );
    }
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut prefix = Vec::with_capacity(SNIFF_BYTES);
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    loop {
        let read = input.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .context("import source size overflow")?;
        if total > maximum_bytes {
            bail!("import source grew beyond the {maximum_bytes} byte limit");
        }
        if prefix.len() < SNIFF_BYTES {
            let available = (SNIFF_BYTES - prefix.len()).min(read);
            prefix.extend_from_slice(&buffer[..available]);
        }
        hasher.update(&buffer[..read]);
    }
    if total != metadata.len() {
        bail!("import source changed size while it was being read");
    }
    Ok(HashedSource {
        sha256: hex::encode(hasher.finalize()),
        size: total,
        prefix,
    })
}

async fn install_temporary(temporary: &Path, destination: &Path) -> Result<bool> {
    // A hard link is an atomic create-if-absent operation. Unlike rename it
    // cannot replace content installed by another writer.
    let created = match fs::hard_link(temporary, destination).await {
        Ok(()) => true,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => false,
        Err(error) => {
            let _ = fs::remove_file(temporary).await;
            return Err(error.into());
        }
    };
    let _ = fs::remove_file(temporary).await;
    Ok(created)
}

async fn verify_existing_content(path: &Path, digest: &str, expected_size: u64) -> Result<()> {
    let mut file = open_read_no_follow(path)
        .await
        .with_context(|| format!("open managed digest {digest} without following links"))?;
    let mut metadata = file.metadata().await?;
    if !metadata.is_file() || metadata.len() != expected_size {
        bail!("managed content at digest {digest} failed its integrity check");
    }
    // A content file is installed with a temporary hard link which is removed
    // immediately. A persistent extra link would let another pathname mutate
    // bytes behind the content address.
    for _ in 0..5 {
        if has_single_link(&metadata) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        metadata = file.metadata().await?;
    }
    if !has_single_link(&metadata) {
        bail!("managed content at digest {digest} has an unsafe hard link");
    }
    file.seek(SeekFrom::Start(0)).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if hex::encode(hasher.finalize()) != digest {
        bail!("managed content at digest {digest} failed its SHA-256 integrity check");
    }
    Ok(())
}

#[cfg(unix)]
fn has_single_link(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink() == 1
}

#[cfg(windows)]
fn has_single_link(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.number_of_links().unwrap_or(0) == 1
}

#[cfg(not(any(unix, windows)))]
fn has_single_link(_metadata: &std::fs::Metadata) -> bool {
    true
}

pub(crate) async fn create_private_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .await?;
    set_private_file(path).await?;
    Ok(file)
}

fn validate_digest(digest: &str) -> Result<()> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("managed content digest is not lowercase SHA-256");
    }
    Ok(())
}

fn digest_path(root: &Path, digest: &str) -> PathBuf {
    root.join(&digest[..2]).join(&digest[2..])
}

async fn ensure_beneath(root: &Path, path: &Path) -> Result<()> {
    let canonical_root = fs::canonicalize(root).await?;
    let canonical_path = fs::canonicalize(path).await?;
    if !canonical_path.starts_with(&canonical_root) {
        bail!(
            "data path {} escapes content root {} through a symbolic link",
            path.display(),
            root.display()
        );
    }
    Ok(())
}

#[cfg(unix)]
async fn set_private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
    Ok(())
}

#[cfg(unix)]
async fn set_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn set_private_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(not(unix))]
async fn set_private_file(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn content_is_addressed_and_deduplicated() {
        let temp = tempfile::tempdir().unwrap();
        let layout = DataLayout::initialize(temp.path()).await.unwrap();
        let first = layout.put_media(b"same bytes").await.unwrap();
        let second = layout.put_media(b"same bytes").await.unwrap();
        assert_eq!(first, second);
        assert_eq!(tokio::fs::read(first.path).await.unwrap(), b"same bytes");
    }

    #[tokio::test]
    async fn rejects_preseeded_same_size_content_with_the_wrong_digest() {
        let temp = tempfile::tempdir().unwrap();
        let layout = DataLayout::initialize(temp.path()).await.unwrap();
        let expected = b"GOOD";
        let digest = hex::encode(Sha256::digest(expected));
        let destination = layout.media.join(&digest[..2]).join(&digest[2..]);
        tokio::fs::create_dir_all(destination.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&destination, b"EVIL").await.unwrap();

        let error = layout.put_media(expected).await.unwrap_err();
        assert!(error.to_string().contains("SHA-256 integrity"));
        assert_eq!(tokio::fs::read(destination).await.unwrap(), b"EVIL");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_preseeded_hard_link() {
        let temp = tempfile::tempdir().unwrap();
        let layout = DataLayout::initialize(temp.path().join("data"))
            .await
            .unwrap();
        let bytes = b"linked bytes";
        let digest = hex::encode(Sha256::digest(bytes));
        let destination = layout.media.join(&digest[..2]).join(&digest[2..]);
        tokio::fs::create_dir_all(destination.parent().unwrap())
            .await
            .unwrap();
        let outside = temp.path().join("outside");
        tokio::fs::write(&outside, bytes).await.unwrap();
        tokio::fs::hard_link(&outside, &destination).await.unwrap();

        let error = layout.put_media(bytes).await.unwrap_err();
        assert!(error.to_string().contains("unsafe hard link"));
    }

    #[tokio::test]
    async fn file_import_streams_and_deduplicates() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.bin");
        let bytes = vec![0x5a; COPY_BUFFER_BYTES + 17];
        tokio::fs::write(&source, &bytes).await.unwrap();
        let layout = DataLayout::initialize(temp.path().join("data"))
            .await
            .unwrap();

        let first = layout
            .put_media_file(&source, bytes.len() as u64)
            .await
            .unwrap();
        let second = layout
            .put_media_file(&source, bytes.len() as u64)
            .await
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(
            layout.media_content(&first.sha256).await.unwrap(),
            Some(first)
        );
    }

    #[tokio::test]
    async fn file_import_enforces_size_limit() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.bin");
        tokio::fs::write(&source, b"too large").await.unwrap();
        let layout = DataLayout::initialize(temp.path().join("data"))
            .await
            .unwrap();
        let error = layout.put_media_file(&source, 2).await.unwrap_err();
        assert!(error.to_string().contains("exceeding"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_content_directories_that_escape_through_symlinks() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("media")).unwrap();
        let error = DataLayout::initialize(&root).await.unwrap_err();
        assert!(error.to_string().contains("escapes content root"));
    }
}
