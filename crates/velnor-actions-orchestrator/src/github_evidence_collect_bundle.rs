use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::OrchestratorError;
use crate::internal::internal;

pub(super) struct BundleRoot {
    path: PathBuf,
    keep: bool,
}

impl BundleRoot {
    pub(super) fn create() -> Result<Self, OrchestratorError> {
        let runner_temp = std::env::var_os("RUNNER_TEMP")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| internal("github_evidence_runner_temp"))?;
        Self::create_at(&runner_temp)
    }

    pub(super) fn create_at(runner_temp: &Path) -> Result<Self, OrchestratorError> {
        if !runner_temp.is_absolute() || !real_directory(runner_temp) {
            return Err(internal("github_evidence_runner_temp"));
        }
        let velnor = runner_temp.join("velnor");
        match fs::symlink_metadata(&velnor) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&velnor)
                    .map_err(|_| internal("github_evidence_runner_directory"))?;
            }
            Ok(_) | Err(_) => return Err(internal("github_evidence_runner_directory")),
        }
        let path = velnor.join("github-evidence-bundle");
        fs::create_dir(&path).map_err(|_| internal("github_evidence_bundle_exists"))?;
        fs::create_dir(path.join("documents"))
            .and_then(|()| fs::create_dir(path.join("archives")))
            .map_err(|_| internal("github_evidence_bundle_create"))?;
        Ok(Self { path, keep: false })
    }

    pub(super) fn path_for(&self, relative: &str) -> Result<PathBuf, OrchestratorError> {
        if !safe_relative(relative) {
            return Err(internal("github_evidence_bundle_path"));
        }
        Ok(self.path.join(relative))
    }

    pub(super) fn write_new(&self, relative: &str, bytes: &[u8]) -> Result<(), OrchestratorError> {
        let path = self.path_for(relative)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| internal("github_evidence_bundle_write"))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| internal("github_evidence_bundle_write"))
    }

    pub(super) fn finish(mut self) {
        self.keep = true;
    }
}

impl Drop for BundleRoot {
    fn drop(&mut self) {
        if !self.keep {
            drop(fs::remove_dir_all(&self.path));
        }
    }
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    encode_hex(&Sha256::digest(bytes))
}

pub(super) fn sha256_file(path: &Path) -> Result<(u64, String), OrchestratorError> {
    use std::io::Read;
    let mut file = File::open(path).map_err(|_| internal("github_evidence_archive_read"))?;
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| internal("github_evidence_archive_read"))?;
        if count == 0 {
            break;
        }
        size = size.saturating_add(count as u64);
        hash.update(&buffer[..count]);
    }
    Ok((size, encode_hex(&hash.finalize())))
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        output.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    output
}

fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && !path.contains('\\')
}
