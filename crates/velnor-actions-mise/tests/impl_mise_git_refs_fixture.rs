//! Owned ref namespace fixtures for Discovery Git integration tests.

#[cfg(test)]
#[path = "impl_mise_git_index_fixture.rs"]
mod base;

pub(crate) use base::git_fixture;
pub(crate) use base::{git_output, git_owned, repo};

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

static MUTATION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write one direct ref through the sterile fixture Git authority.
pub(crate) fn set_ref(root: &Path, name: &str, oid: &str) -> Result<(), String> {
    git_owned(
        root,
        vec!["update-ref".to_owned(), name.to_owned(), oid.to_owned()],
    )
}

/// Write one symbolic ref through the sterile fixture Git authority.
pub(crate) fn set_symbolic_ref(root: &Path, name: &str, target: &str) -> Result<(), String> {
    git_owned(
        root,
        vec![
            "symbolic-ref".to_owned(),
            name.to_owned(),
            target.to_owned(),
        ],
    )
}

/// Install the remote-tracking branch and its exact origin HEAD symref.
pub(crate) fn set_origin_head(root: &Path, branch: &str) -> Result<String, String> {
    let oid = git_output(root, &["rev-parse", "HEAD"])?;
    let oid = oid.trim().to_owned();
    let target = format!("refs/remotes/origin/{branch}");
    set_ref(root, &target, &oid)?;
    set_symbolic_ref(root, "refs/remotes/origin/HEAD", &target)?;
    Ok(oid)
}

/// A process-unique valid branch name for a mutation watcher.
pub(crate) fn unique_branch(label: &str) -> String {
    let count = MUTATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("velnor-{label}-{}-{count}", std::process::id())
}

/// Best-effort bounded observer for private ref installation, then one source mutation.
///
/// This filesystem observer coordinates a race test without adding a product hook;
/// it is not a deterministic phase oracle.
pub(crate) fn mutate_after_private_ref(
    root: &Path,
    branch: &str,
    replacement: &str,
) -> JoinHandle<Result<(), String>> {
    let root = root.to_owned();
    let branch = branch.to_owned();
    let replacement = replacement.to_owned();
    std::thread::spawn(move || {
        let base = Path::new("/tmp")
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if private_ref_exists(&base, &branch)? {
                let name = format!("refs/heads/{branch}");
                return git_owned(
                    &root,
                    vec!["update-ref".to_owned(), name, replacement.clone()],
                );
            }
            if Instant::now() >= deadline {
                return Err("private ref installation was not observed".to_owned());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    })
}

fn private_ref_exists(base: &Path, branch: &str) -> Result<bool, String> {
    let entries = std::fs::read_dir(base).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .path()
            .join("gitdir")
            .join("refs")
            .join("heads")
            .join(branch)
            .is_file()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Run a raw fixture Git request without converting a native failure to test setup failure.
pub(crate) fn raw_output(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    git_fixture::command(root)
        .map_err(|error| error.to_string())?
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| error.to_string())
}
