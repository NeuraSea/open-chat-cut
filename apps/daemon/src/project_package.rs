use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use openchatcut_domain::ProjectEnvelope;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::fs::File;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::content_store::{
    DataLayout, HashedSource, create_private_file, hash_open_file, open_read_no_follow,
};

pub const PROJECT_PACKAGE_FORMAT: &str = "openchatcut-project-package";
pub const PROJECT_PACKAGE_VERSION: u32 = 1;
pub const MAX_PACKAGE_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
const MAX_PACKAGE_ENTRIES: usize = 100_002;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_ENVELOPE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectPackageManifest {
    pub format: String,
    pub version: u32,
    pub project_id: String,
    pub revision: u64,
    pub document_hash: Value,
    pub envelope_path: String,
    pub media: Vec<ProjectPackageMedia>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectPackageMedia {
    pub sha256: String,
    pub byte_size: u64,
    pub path: String,
    pub asset_ids: Vec<String>,
}

#[derive(Debug)]
pub struct ProjectPackageArtifact {
    pub temporary_path: PathBuf,
    pub sha256: String,
    pub byte_size: u64,
    pub media_count: usize,
}

#[derive(Debug)]
pub struct ExtractedProjectPackage {
    pub manifest: ProjectPackageManifest,
    pub envelope: ProjectEnvelope,
    pub media: Vec<ExtractedPackageMedia>,
}

#[derive(Debug)]
pub struct ExtractedPackageMedia {
    pub temporary_path: PathBuf,
    pub hashed: HashedSource,
}

struct PackageSource {
    path: String,
    file: std::fs::File,
}

pub async fn create_project_package(
    layout: &DataLayout,
    envelope: &ProjectEnvelope,
) -> Result<ProjectPackageArtifact> {
    let mut media = BTreeMap::<String, ProjectPackageMedia>::new();
    for asset in &envelope.document.assets {
        for (digest, reference) in asset_media_references(asset)? {
            let content = layout
                .media_content(&digest)
                .await?
                .with_context(|| format!("managed media {digest} is missing"))?;
            let entry = media
                .entry(digest.clone())
                .or_insert_with(|| ProjectPackageMedia {
                    sha256: digest.clone(),
                    byte_size: content.size,
                    path: format!("media/sha256/{digest}"),
                    asset_ids: Vec::new(),
                });
            if entry.byte_size != content.size {
                bail!("managed content size changed while package was planned");
            }
            entry.asset_ids.push(reference);
        }
    }
    for entry in media.values_mut() {
        entry.asset_ids.sort();
        entry.asset_ids.dedup();
    }
    let manifest = ProjectPackageManifest {
        format: PROJECT_PACKAGE_FORMAT.to_owned(),
        version: PROJECT_PACKAGE_VERSION,
        project_id: envelope.document.id.to_string(),
        revision: envelope.revision,
        document_hash: serde_json::to_value(&envelope.document_hash)?,
        envelope_path: "project/envelope.json".to_owned(),
        media: media.values().cloned().collect(),
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let envelope_bytes = serde_json::to_vec_pretty(envelope)?;
    let mut sources = Vec::with_capacity(media.len());
    for entry in media.values() {
        let content = layout
            .media_content(&entry.sha256)
            .await?
            .context("managed content disappeared while package was opened")?;
        let file = open_read_no_follow(&content.path).await?.into_std().await;
        sources.push(PackageSource {
            path: entry.path.clone(),
            file,
        });
    }

    let temporary_path = layout
        .temporary
        .join(format!(".project-package-{}.tmp", uuid::Uuid::new_v4()));
    let output = create_private_file(&temporary_path).await?.into_std().await;
    let media_count = sources.len();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut archive = ZipWriter::new(output);
        let text_options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o600);
        let media_options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(0o600);
        archive.start_file("manifest.json", text_options)?;
        archive.write_all(&manifest_bytes)?;
        archive.start_file("project/envelope.json", text_options)?;
        archive.write_all(&envelope_bytes)?;
        for mut source in sources {
            archive.start_file(&source.path, media_options)?;
            std::io::copy(&mut source.file, &mut archive)?;
        }
        let output = archive.finish()?;
        output.sync_all()?;
        Ok(())
    })
    .await
    .context("join project package writer")??;

    let mut package = File::open(&temporary_path).await?;
    let hashed = hash_open_file(&mut package, MAX_PACKAGE_BYTES).await?;
    Ok(ProjectPackageArtifact {
        temporary_path,
        sha256: hashed.sha256,
        byte_size: hashed.size,
        media_count,
    })
}

pub async fn extract_project_package(
    package: File,
    temporary_directory: &Path,
) -> Result<ExtractedProjectPackage> {
    let package = package.into_std().await;
    let temporary_directory = temporary_directory.to_owned();
    tokio::task::spawn_blocking(move || extract_project_package_sync(package, &temporary_directory))
        .await
        .context("join project package extractor")?
}

fn extract_project_package_sync(
    package: std::fs::File,
    temporary_directory: &Path,
) -> Result<ExtractedProjectPackage> {
    let mut archive = zip::ZipArchive::new(package).context("open project package ZIP")?;
    if archive.len() == 0 || archive.len() > MAX_PACKAGE_ENTRIES {
        bail!("project package entry count is outside the safe limit");
    }
    let mut names = BTreeSet::new();
    let mut total_uncompressed = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        validate_package_entry(&entry)?;
        if !names.insert(entry.name().to_owned()) {
            bail!("project package contains duplicate entry names");
        }
        total_uncompressed = total_uncompressed
            .checked_add(entry.size())
            .context("project package uncompressed size overflow")?;
        if total_uncompressed > MAX_PACKAGE_BYTES {
            bail!("project package exceeds the 1 TiB uncompressed limit");
        }
    }
    if !names.contains("manifest.json") || !names.contains("project/envelope.json") {
        bail!("project package is missing its manifest or envelope");
    }
    let manifest: ProjectPackageManifest = read_json_entry(
        &mut archive,
        "manifest.json",
        MAX_MANIFEST_BYTES,
        "manifest",
    )?;
    if manifest.format != PROJECT_PACKAGE_FORMAT || manifest.version != PROJECT_PACKAGE_VERSION {
        bail!("unsupported OpenChatCut project package format/version");
    }
    if manifest.envelope_path != "project/envelope.json" {
        bail!("project package envelopePath is invalid");
    }
    let envelope: ProjectEnvelope = read_json_entry(
        &mut archive,
        "project/envelope.json",
        MAX_ENVELOPE_BYTES,
        "envelope",
    )?;
    let canonical = ProjectEnvelope::new(envelope.document.clone())
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if canonical.document_hash != envelope.document_hash
        || manifest.document_hash != serde_json::to_value(&envelope.document_hash)?
        || manifest.project_id != envelope.document.id.as_str()
        || manifest.revision != envelope.revision
    {
        bail!("project package envelope does not match its manifest/canonical hash");
    }
    let mut expected = BTreeMap::<String, Vec<String>>::new();
    for asset in &envelope.document.assets {
        for (digest, reference) in asset_media_references(asset)? {
            expected.entry(digest).or_default().push(reference);
        }
    }
    for ids in expected.values_mut() {
        ids.sort();
        ids.dedup();
    }
    if manifest.media.len() != expected.len() {
        bail!("project package media manifest does not match the envelope");
    }
    let mut media_by_digest = BTreeMap::new();
    for media in &manifest.media {
        validate_digest(&media.sha256)?;
        if media.path != format!("media/sha256/{}", media.sha256)
            || media.byte_size > MAX_PACKAGE_BYTES
        {
            bail!("project package media entry metadata is invalid");
        }
        let mut asset_ids = media.asset_ids.clone();
        asset_ids.sort();
        asset_ids.dedup();
        if expected.get(&media.sha256) != Some(&asset_ids) {
            bail!("project package media asset mapping is invalid");
        }
        if media_by_digest
            .insert(media.sha256.clone(), media)
            .is_some()
        {
            bail!("project package repeats a media digest");
        }
    }
    for name in &names {
        if matches!(name.as_str(), "manifest.json" | "project/envelope.json") {
            continue;
        }
        let Some(digest) = name.strip_prefix("media/sha256/") else {
            bail!("project package contains an unsupported entry");
        };
        if !media_by_digest.contains_key(digest) {
            bail!("project package contains media not declared by the manifest");
        }
    }
    if names.len() != manifest.media.len() + 2 {
        bail!("project package is missing declared media entries");
    }

    let mut extracted_paths = Vec::new();
    let result = (|| -> Result<Vec<ExtractedPackageMedia>> {
        let mut extracted = Vec::with_capacity(manifest.media.len());
        for media in &manifest.media {
            let mut entry = archive.by_name(&media.path)?;
            if entry.size() != media.byte_size {
                bail!("project package media size does not match its manifest");
            }
            let temporary_path = temporary_directory.join(format!(
                ".package-media-{}-{}.tmp",
                &media.sha256[..16],
                uuid::Uuid::new_v4()
            ));
            let mut output = create_private_std_file(&temporary_path)?;
            extracted_paths.push(temporary_path.clone());
            let mut hasher = Sha256::new();
            let mut prefix = Vec::with_capacity(512);
            let mut total = 0_u64;
            let mut buffer = vec![0_u8; 1024 * 1024];
            loop {
                let read = entry.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                total = total
                    .checked_add(read as u64)
                    .context("media size overflow")?;
                if total > media.byte_size {
                    bail!("project package media expanded beyond its declared size");
                }
                let remaining = 512_usize.saturating_sub(prefix.len());
                prefix.extend_from_slice(&buffer[..read.min(remaining)]);
                hasher.update(&buffer[..read]);
                output.write_all(&buffer[..read])?;
            }
            output.flush()?;
            output.sync_all()?;
            if total != media.byte_size || hex::encode(hasher.finalize()) != media.sha256 {
                bail!("project package media digest does not match its entry name");
            }
            extracted.push(ExtractedPackageMedia {
                temporary_path,
                hashed: HashedSource {
                    sha256: media.sha256.clone(),
                    size: total,
                    prefix,
                },
            });
        }
        Ok(extracted)
    })();
    let media = match result {
        Ok(media) => media,
        Err(error) => {
            for path in extracted_paths {
                let _ = std::fs::remove_file(path);
            }
            return Err(error);
        }
    };
    Ok(ExtractedProjectPackage {
        manifest,
        envelope,
        media,
    })
}

fn validate_package_entry(entry: &zip::read::ZipFile<'_, std::fs::File>) -> Result<()> {
    let path = Path::new(entry.name());
    if entry.encrypted() {
        bail!("project package encrypted entries are forbidden");
    }
    if entry.is_dir()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || entry.name().contains('\\')
        || entry.name().contains('\0')
    {
        bail!("project package contains an unsafe entry path");
    }
    if entry
        .unix_mode()
        .is_some_and(|mode| mode & 0o170000 == 0o120000)
    {
        bail!("project package symlink entries are forbidden");
    }
    let is_media = entry.name().starts_with("media/sha256/");
    if is_media && entry.compression() != CompressionMethod::Stored {
        bail!("project package media entries must use stored compression");
    }
    if !is_media
        && !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        )
    {
        bail!("project package metadata uses an unsupported compression method");
    }
    if entry.size() > 0
        && (entry.compressed_size() == 0 || entry.size() / entry.compressed_size().max(1) > 200)
    {
        bail!("project package entry exceeds the safe compression ratio");
    }
    if !matches!(entry.name(), "manifest.json" | "project/envelope.json") {
        let digest = entry
            .name()
            .strip_prefix("media/sha256/")
            .context("project package contains an unsupported entry")?;
        validate_digest(digest)?;
    }
    Ok(())
}

fn read_json_entry<T: for<'de> Deserialize<'de>>(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
    maximum_bytes: u64,
    label: &str,
) -> Result<T> {
    let mut entry = archive.by_name(name)?;
    if entry.size() > maximum_bytes {
        bail!("project package {label} exceeds its size limit");
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .by_ref()
        .take(maximum_bytes + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum_bytes {
        bail!("project package {label} exceeds its size limit");
    }
    serde_json::from_slice(&bytes).with_context(|| format!("parse project package {label}"))
}

fn validate_digest(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("project package contains an invalid SHA-256 digest");
    }
    Ok(())
}

fn asset_media_references(asset: &openchatcut_domain::Asset) -> Result<Vec<(String, String)>> {
    let digest = asset.content_hash.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "asset {} has no managed content and makes the project non-portable",
            asset.id
        )
    })?;
    let mut references = vec![(digest.as_str().to_owned(), asset.id.to_string())];
    if let Some(derivatives) = asset
        .extensions
        .get("derivatives")
        .and_then(Value::as_object)
    {
        for kind in ["thumbnail", "contactSheet", "waveform", "proxy", "audio"] {
            let Some(metadata) = derivatives.get(kind).and_then(Value::as_object) else {
                continue;
            };
            let digest = metadata
                .get("contentHash")
                .and_then(Value::as_str)
                .with_context(|| format!("asset {} {kind} derivative has no hash", asset.id))?;
            validate_digest(digest)?;
            references.push((digest.to_owned(), format!("{}#derivative:{kind}", asset.id)));
        }
    }
    Ok(references)
}

fn create_private_std_file(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("create private package extraction file {}", path.display()))
}
