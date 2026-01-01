//! Explicit repository authority for test fixture Git subprocesses.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Build Git against the requested checkout, without ambient process authority.
pub(crate) fn command(root: &Path) -> io::Result<Command> {
    let root = root.canonicalize()?;
    let git_dir = metadata_dir(&root)?;
    let mut command = sterile_command();
    command
        .current_dir(&root)
        .arg("--git-dir")
        .arg(git_dir)
        .arg("--work-tree")
        .arg(root);
    Ok(command)
}

/// Build a fixture Git process before assigning repository authority.
pub(crate) fn sterile_command() -> Command {
    let mut command = Command::new("git");
    command.env_clear();
    for key in ["PATH", "SYSTEMROOT", "TMPDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", null)
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_TEMPLATE_DIR", "")
        .args(["-c", &format!("core.hooksPath={null}")])
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(["-c", "core.fsmonitor=false", "-c", "init.templateDir="]);
    command
}

/// Existing linked-worktree fixtures carry a Git-created metadata pointer.
fn metadata_dir(root: &Path) -> io::Result<PathBuf> {
    let metadata = root.join(".git");
    if !metadata.is_file() {
        return Ok(metadata);
    }
    let pointer = std::fs::read_to_string(&metadata)?;
    let path = pointer
        .trim_end()
        .strip_prefix("gitdir: ")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::other("invalid fixture Git metadata pointer"))?;
    root.join(path).canonicalize()
}
