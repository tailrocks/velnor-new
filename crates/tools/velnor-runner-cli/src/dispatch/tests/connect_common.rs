use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::super::connect::{ConnectFileOps, ConnectRequest};

pub(super) fn file_ops() -> ConnectFileOps {
    use super::super::connect::ConnectError;

    ConnectFileOps {
        read: |path| match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                std::fs::read_to_string(path)
                    .map(Some)
                    .map_err(|_| ConnectError::Write)
            }
            Ok(_) => Err(ConnectError::Config),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(ConnectError::Write),
        },
        persist: |path, text| {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut options = std::fs::OpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .mode(if cfg!(target_os = "linux") {
                    0o640
                } else {
                    0o600
                });
            let mut file = options.open(path).map_err(|_| ConnectError::Write)?;
            file.write_all(text.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|_| ConnectError::Write)
        },
        remove: |path, expected| {
            let contents = std::fs::read_to_string(path).map_err(|_| ConnectError::Write)?;
            if contents != expected {
                return Err(ConnectError::Write);
            }
            std::fs::remove_file(path).map_err(|_| ConnectError::Write)
        },
    }
}

pub(super) fn allowed_events() -> Vec<String> {
    vec!["push".to_owned(), "pull_request".to_owned()]
}

pub(super) fn allowed_workflow_paths() -> Vec<String> {
    vec![".github/workflows/ci.yml".to_owned()]
}

pub(super) fn request<'a>(
    config_path: &'a Path,
    events: &'a [String],
    workflow_paths: &'a [String],
) -> ConnectRequest<'a> {
    ConnectRequest {
        config_path,
        repo: "example/repo",
        scale_set: target_set(),
        platform: "linux/amd64",
        host_platform: Some(target_host()),
        registration_scope: Some("repository"),
        runner_group_id: Some(1),
        runner_group_name: Some("Default"),
        allowed_events: events,
        allowed_workflow_paths: workflow_paths,
        image_profile: target_profile(),
        max_jobs: Some(1),
        drain_timeout_secs: Some(1800),
        docker_context: Some("system"),
        endpoint: Some("unix:///var/run/docker.sock"),
    }
}

pub(super) fn expected_credential_ref() -> &'static str {
    if cfg!(target_os = "linux") {
        "systemd-credential:github-token"
    } else {
        "keychain:com.tailrocks.velnor.host/velnor-host"
    }
}

fn target_host() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else {
        "macos"
    }
}

fn target_set() -> &'static str {
    if cfg!(target_os = "linux") {
        "ubuntu-24.04-scale-set"
    } else {
        "ubuntu-26.04-scale-set"
    }
}

fn target_profile() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        Some("ubuntu-24.04-amd64")
    } else {
        None
    }
}

pub(super) struct TempDir(PathBuf);

impl TempDir {
    pub(super) fn new(label: &str) -> Result<Self, String> {
        static TICK: AtomicU64 = AtomicU64::new(0);
        let n = TICK.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-connect-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir(&path).map_err(|err| err.to_string())?;
        Ok(Self(path))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}
