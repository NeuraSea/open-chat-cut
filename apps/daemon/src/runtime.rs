use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{Config, DEFAULT_PROTOCOL_VERSION};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDescriptor {
    pub protocol_version: String,
    pub instance_id: String,
    pub api_base_url: String,
    pub token_path: PathBuf,
    pub pid: u32,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct RuntimeFiles {
    pub descriptor: RuntimeDescriptor,
    descriptor_path: PathBuf,
}

impl RuntimeFiles {
    pub fn install(config: &Config, bound_address: SocketAddr, token: &str) -> Result<Self> {
        let token_path = absolute_path(&config.token_path)?;
        let descriptor_path = absolute_path(&config.runtime_descriptor)?;
        write_private_atomic(&token_path, format!("{token}\n").as_bytes())?;

        let descriptor = RuntimeDescriptor {
            protocol_version: DEFAULT_PROTOCOL_VERSION.to_owned(),
            instance_id: uuid::Uuid::new_v4().to_string(),
            api_base_url: config.api_base_url(bound_address),
            token_path,
            pid: std::process::id(),
            started_at: Utc::now(),
        };
        let bytes = serde_json::to_vec_pretty(&descriptor)?;
        write_private_atomic(&descriptor_path, &bytes)?;
        Ok(Self {
            descriptor,
            descriptor_path,
        })
    }

    pub fn cleanup(&self) {
        let belongs_to_us = fs::read(&self.descriptor_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<RuntimeDescriptor>(&bytes).ok())
            .is_some_and(|descriptor| descriptor.instance_id == self.descriptor.instance_id);
        if belongs_to_us
            && let Err(error) = fs::remove_file(&self.descriptor_path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error, path = %self.descriptor_path.display(), "remove runtime descriptor");
        }
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("runtime file has no parent")?;
    let parent_existed = parent.exists();
    fs::create_dir_all(parent)
        .with_context(|| format!("create runtime directory {}", parent.display()))?;
    if !parent_existed {
        set_private_directory_permissions(parent)?;
    }

    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("runtime"),
        uuid::Uuid::new_v4()
    ));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("create {}", temporary.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    install_atomic(&temporary, path)
        .with_context(|| format!("install runtime file {}", path.display()))?;
    set_private_file_permissions(path)?;
    Ok(())
}

#[cfg(unix)]
fn install_atomic(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(temporary, destination)
}

#[cfg(not(unix))]
fn install_atomic(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    // std::fs::rename cannot replace a file on Windows. Runtime files are
    // discovery hints (the token itself remains protected), so use the narrow
    // remove/rename fallback instead of failing every daemon restart.
    match fs::remove_file(destination) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::rename(temporary, destination)
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_never_contains_the_token() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config::for_test(temp.path().to_owned());
        let files = RuntimeFiles::install(&config, "127.0.0.1:3210".parse().unwrap(), "top-secret")
            .unwrap();
        let descriptor = fs::read_to_string(&config.runtime_descriptor).unwrap();
        assert!(!descriptor.contains("top-secret"));
        assert_eq!(
            fs::read_to_string(&config.token_path).unwrap().trim(),
            "top-secret"
        );
        assert!(files.descriptor.api_base_url.ends_with("/api/v1"));
        files.cleanup();
        assert!(!config.runtime_descriptor.exists());
    }
}
