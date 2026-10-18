//! Isolated GitHub CLI execution for release verification.

use std::ffi::OsString;
use std::fs::{self, DirBuilder};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use uuid::Uuid;

use super::ProviderFailure;

const GH_VERSION: &str = "2.102.0";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

pub(super) struct Workspace {
    executable: PathBuf,
    root: PathBuf,
    assets: PathBuf,
    config: PathBuf,
}

impl Workspace {
    pub(super) fn create() -> Result<Self, ProviderFailure> {
        let executable = resolve_executable()?;
        let root = std::env::temp_dir().join(format!("velnor-resource-probe-{}", Uuid::new_v4()));
        create_private_dir(&root)?;
        let assets = root.join("assets");
        let config = root.join("gh-config");
        if create_private_dir(&assets)
            .and_then(|()| create_private_dir(&config))
            .is_err()
        {
            remove_workspace(&root);
            return Err(ProviderFailure::GhUnavailable);
        }
        Ok(Self {
            executable,
            root,
            assets,
            config,
        })
    }

    pub(super) fn assets(&self) -> &Path {
        &self.assets
    }

    fn config(&self) -> &Path {
        &self.config
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn executable(&self) -> &Path {
        &self.executable
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        remove_workspace(&self.root);
    }
}

fn remove_workspace(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path) {
        eprintln!("resource_probe=workspace_cleanup_failed reason={error}");
    }
}

pub(super) async fn assert_version(
    workspace: &Workspace,
    pat: &str,
) -> Result<(), ProviderFailure> {
    let output = run(workspace, pat, [OsString::from("--version")]).await?;
    let first_line = output
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    let expected = format!("gh version {GH_VERSION}");
    let valid_version_line = first_line
        .strip_prefix(expected.as_bytes())
        .is_some_and(|suffix| suffix.first().is_none_or(u8::is_ascii_whitespace));
    if valid_version_line {
        Ok(())
    } else {
        Err(ProviderFailure::GhVersion)
    }
}

pub(super) async fn run<I>(
    workspace: &Workspace,
    pat: &str,
    arguments: I,
) -> Result<Vec<u8>, ProviderFailure>
where
    I: IntoIterator<Item = OsString>,
{
    let mut command = Command::new(workspace.executable());
    command
        .args(arguments)
        .env_clear()
        .env("GH_TOKEN", pat)
        .env("GH_CONFIG_DIR", workspace.config())
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_PAGER", "cat")
        .env("PAGER", "cat")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .env("HOME", workspace.root())
        .env("TMPDIR", workspace.root())
        .env("TMP", workspace.root())
        .env("TEMP", workspace.root())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| ProviderFailure::GhUnavailable)?;
    let stdout = child.stdout.take().ok_or(ProviderFailure::GhUnavailable)?;
    let output = tokio::time::timeout(COMMAND_TIMEOUT, async {
        let (stdout, status) = tokio::join!(read_bounded(stdout), child.wait());
        let stdout = stdout?;
        let status = status.map_err(|_| ProviderFailure::GhUnavailable)?;
        Ok::<_, ProviderFailure>((stdout, status))
    })
    .await
    .map_err(|_| ProviderFailure::GhTimeout)?;
    let (stdout, status) = output?;
    if !status.success() {
        return Err(ProviderFailure::GhCommand);
    }
    Ok(stdout)
}

async fn read_bounded<R>(reader: R) -> Result<Vec<u8>, ProviderFailure>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(MAX_OUTPUT_BYTES);
    reader
        .take((MAX_OUTPUT_BYTES as u64) + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| ProviderFailure::GhOutput)?;
    if bytes.len() > MAX_OUTPUT_BYTES {
        return Err(ProviderFailure::GhOutput);
    }
    Ok(bytes)
}

fn resolve_executable() -> Result<PathBuf, ProviderFailure> {
    let path = std::env::var_os("PATH").ok_or(ProviderFailure::GhUnavailable)?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join("gh");
        let Ok(executable) = candidate.canonicalize() else {
            continue;
        };
        if is_executable(&executable) {
            return Ok(executable);
        }
    }
    Err(ProviderFailure::GhUnavailable)
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn create_private_dir(path: &Path) -> Result<(), ProviderFailure> {
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|_| ProviderFailure::GhUnavailable)
}
