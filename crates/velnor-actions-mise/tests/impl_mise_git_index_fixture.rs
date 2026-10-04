//! Owned Git fixtures for private-index integration tests.

#[cfg(test)]
#[path = "../../test_support/git_fixture.rs"]
pub(crate) mod git_fixture;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A canonical, uniquely named fixture root removed on drop.
pub(crate) struct Fixture {
    pub(crate) root: PathBuf,
}

impl Fixture {
    pub(crate) fn new(label: &str) -> Result<Self, String> {
        let base = std::env::temp_dir()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        for _attempt in 0..32 {
            let count = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!(
                "velnor-mise-git-index-{label}-{}-{count}",
                std::process::id()
            );
            let root = base.join(name);
            match std::fs::create_dir(&root) {
                Ok(()) => {
                    return Ok(Self {
                        root: root.canonicalize().map_err(|error| error.to_string())?,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("fixture directory collision limit".to_owned())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!(
                "fixture cleanup failed for {}: {error}",
                self.root.display()
            ),
        }
    }
}

/// Run one fixture Git command with explicit repository routing.
pub(crate) fn git(root: &Path, args: &[&str]) -> Result<(), String> {
    git_owned(root, args.iter().map(|arg| (*arg).to_owned()).collect())
}

/// Run one fixture Git command with owned arguments.
pub(crate) fn git_owned(root: &Path, args: Vec<String>) -> Result<(), String> {
    let status = git_fixture::command(root)
        .map_err(|error| error.to_string())?
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "fixture git failed in {}: {status}",
            root.display()
        ))
    }
}

/// Run one fixture Git command and return UTF-8 standard output.
pub(crate) fn git_output(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = git_fixture::command(root)
        .map_err(|error| error.to_string())?
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "fixture git failed in {}: {:?}",
            root.display(),
            output.status
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

/// Create a committed repository with a complete index at one requested version.
pub(crate) fn repo(label: &str, object_format: &str, version: u8) -> Result<Fixture, String> {
    let fixture = Fixture::new(label)?;
    let mut init = vec!["init".to_owned(), "--initial-branch=main".to_owned()];
    if object_format == "sha256" {
        init.push("--object-format=sha256".to_owned());
    }
    git_owned(&fixture.root, init)?;
    git(&fixture.root, &["config", "user.email", "test@example.com"])?;
    git(&fixture.root, &["config", "user.name", "Velnor Test"])?;
    git(&fixture.root, &["config", "commit.gpgsign", "false"])?;
    git(&fixture.root, &["config", "core.fsmonitor", "false"])?;
    git(&fixture.root, &["config", "core.splitIndex", "false"])?;
    std::fs::write(fixture.root.join("tracked.txt"), "initial\n")
        .map_err(|error| error.to_string())?;
    let mut add_args = vec!["add".to_owned(), "--".to_owned(), "tracked.txt".to_owned()];
    if version == 3 {
        std::fs::write(fixture.root.join("v3-dummy.txt"), "dummy\n")
            .map_err(|error| error.to_string())?;
        add_args.push("v3-dummy.txt".to_owned());
    }
    git_owned(&fixture.root, add_args)?;
    git(&fixture.root, &["commit", "-m", "initial"])?;
    if version == 3 {
        git(
            &fixture.root,
            &["update-index", "--skip-worktree", "--", "v3-dummy.txt"],
        )?;
    }
    git_owned(
        &fixture.root,
        vec![
            "update-index".to_owned(),
            format!("--index-version={version}"),
        ],
    )?;
    Ok(fixture)
}

/// Create an initialized but unborn repository with no index.
pub(crate) fn unborn(label: &str) -> Result<Fixture, String> {
    let fixture = Fixture::new(label)?;
    git(&fixture.root, &["init", "--initial-branch=main"])?;
    Ok(fixture)
}

/// Resolve the repository metadata directory for normal and linked worktrees.
pub(crate) fn git_dir(root: &Path) -> Result<PathBuf, String> {
    let marker = root.join(".git");
    let metadata = std::fs::symlink_metadata(&marker).map_err(|error| error.to_string())?;
    if metadata.file_type().is_dir() {
        return marker.canonicalize().map_err(|error| error.to_string());
    }
    let pointer = std::fs::read_to_string(&marker).map_err(|error| error.to_string())?;
    let target = pointer
        .trim()
        .strip_prefix("gitdir: ")
        .ok_or_else(|| "invalid linked worktree pointer".to_owned())?;
    let target = Path::new(target);
    let target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        root.join(target)
    };
    target.canonicalize().map_err(|error| error.to_string())
}

/// Install a post-index-change marker hook and return its marker path.
pub(crate) fn install_hook(root: &Path) -> Result<PathBuf, String> {
    let hooks = git_dir(root)?.join("hooks");
    std::fs::create_dir_all(&hooks).map_err(|error| error.to_string())?;
    let marker = root.join("post-index-change.marker");
    let hook = hooks.join("post-index-change");
    let marker_text = shell_quote(&marker);
    std::fs::write(&hook, format!("#!/bin/sh\nprintf hook > {marker_text}\n"))
        .map_err(|error| error.to_string())?;
    set_executable(&hook)?;
    let hooks_text = hooks
        .to_str()
        .ok_or_else(|| "non-utf8 hooks path".to_owned())?;
    git_owned(
        root,
        vec![
            "config".to_owned(),
            "core.hooksPath".to_owned(),
            hooks_text.to_owned(),
        ],
    )?;
    Ok(marker)
}

fn shell_quote(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\'', "'\\''");
    format!("'{value}'")
}

fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .map_err(|error| error.to_string())?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Advance both file timestamps while preserving its bytes.
pub(crate) fn touch_identical(path: &Path) -> Result<(), String> {
    let old = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .modified()
        .map_err(|error| error.to_string())?;
    let timestamp = old
        .checked_add(std::time::Duration::from_secs(2))
        .ok_or_else(|| "file timestamp overflow".to_owned())?;
    let times = std::fs::FileTimes::new()
        .set_accessed(timestamp)
        .set_modified(timestamp);
    std::fs::File::open(path)
        .map_err(|error| error.to_string())?
        .set_times(times)
        .map_err(|error| error.to_string())
}
