//! Tracked symlinks as cross-directory inputs for affected selection.
//!
//! A tracked symlink inside package A that points into package B (or
//! anywhere else) makes A read B's files: a change under the link's
//! target must select A, and so must a change to any link on the way
//! (chained and directory symlinks). Every link resolves component by
//! component with full symlink semantics against the checked-out tree.
//! A link that escapes the repository, dangles, or loops cannot be
//! bounded and fails closed for its owners.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use velnor_actions_mise::GitRequest;

/// Tracked symlink mode in `git ls-files -s`.
const SYMLINK_MODE: &str = "120000";

/// Maximum links followed while resolving one path (Linux `MAXSYMLINKS`).
const MAX_LINK_HOPS: usize = 40;

/// One tracked symlink with its fully resolved target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedLink {
    /// Repository-relative path of the tracked link itself.
    pub(crate) link: String,
    /// Every link path followed, the tracked link first.
    pub(crate) chain: Vec<String>,
    /// Repository-relative real target (file or directory).
    pub(crate) target: String,
}

impl ResolvedLink {
    /// True when changing `path` changes what reading the link yields.
    pub(crate) fn reads(&self, path: &str) -> bool {
        self.chain.iter().any(|link| link == path)
            || path == self.target
            || path.starts_with(&format!("{}/", self.target))
            || self.target.is_empty()
    }
}

/// Outcome of resolving one tracked symlink.
pub(crate) type LinkOutcome = Result<ResolvedLink, (String, String)>;

/// Every tracked symlink under `root`, resolved against the working tree.
///
/// # Errors
///
/// Returns a problem token when the index cannot be listed.
pub(crate) fn tracked_links(root: &Path) -> Result<Vec<LinkOutcome>, String> {
    let args = ["-s", "-z"].iter().map(OsString::from).collect();
    let output = GitRequest::ls_files(args)
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    let mut links = Vec::new();
    for record in output.stdout.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(record).map_err(|_| "non_utf8_index_entry".to_owned())?;
        let Some((meta, path)) = text.split_once('\t') else {
            return Err("malformed_index_entry".to_owned());
        };
        if meta.split(' ').next() != Some(SYMLINK_MODE) {
            continue;
        }
        links.push(resolve_link(root, path).map_err(|problem| (path.to_owned(), problem)));
    }
    Ok(links)
}

/// Resolve one repository-relative symlink path to its real target.
///
/// # Errors
///
/// Returns `escapes_repository`, `dangling`, or `loop` when the target
/// cannot be bounded inside the repository.
pub(crate) fn resolve_link(root: &Path, link: &str) -> Result<ResolvedLink, String> {
    let mut chain = Vec::new();
    let mut current: Vec<String> = link.split('/').map(str::to_owned).collect();
    for _ in 0..MAX_LINK_HOPS {
        let Some((index, prefix)) = first_link_prefix(root, &current) else {
            let target = current.join("/");
            if !root.join(&target).exists() {
                return Err("dangling".to_owned());
            }
            return Ok(ResolvedLink {
                link: link.to_owned(),
                chain,
                target,
            });
        };
        let destination =
            std::fs::read_link(root.join(&prefix)).map_err(|_| "dangling".to_owned())?;
        chain.push(prefix.clone());
        let parent = Path::new(&prefix)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut next = lexical_join(&parent, &destination)?;
        next.extend(current.into_iter().skip(index + 1));
        current = next;
    }
    Err("loop".to_owned())
}

/// First prefix of `components` that is a symlink in the working tree.
fn first_link_prefix(root: &Path, components: &[String]) -> Option<(usize, String)> {
    let mut prefix = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        prefix.push(component);
        let is_link = std::fs::symlink_metadata(root.join(&prefix))
            .is_ok_and(|meta| meta.file_type().is_symlink());
        if is_link {
            return Some((index, components[..=index].join("/")));
        }
    }
    None
}

/// Join `destination` onto repository-relative `base`, lexically.
///
/// Absolute destinations and `..` above the repository root escape.
fn lexical_join(base: &Path, destination: &Path) -> Result<Vec<String>, String> {
    if destination.is_absolute() {
        return Err("escapes_repository".to_owned());
    }
    let mut parts: Vec<String> = base
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    for component in destination.components() {
        match component {
            Component::Normal(name) => parts.push(name.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err("escapes_repository".to_owned());
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("escapes_repository".to_owned());
            }
        }
    }
    Ok(parts)
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    #[test]
    fn chained_directory_links_record_every_hop() {
        let root = tempfile::tempdir().expect("root");
        let base = root.path();
        std::fs::create_dir_all(base.join("app/src/data/real")).expect("dirs");
        std::fs::write(base.join("app/src/data/real/x.sql"), "select 1").expect("file");
        std::fs::create_dir_all(base.join("scripts/m")).expect("dirs");
        symlink("real", base.join("app/src/data/link")).expect("inner link");
        symlink("../../app/src/data/link", base.join("scripts/m/fixtures")).expect("outer link");
        let resolved = resolve_link(base, "scripts/m/fixtures").expect("resolves");
        assert_eq!(resolved.target, "app/src/data/real");
        assert_eq!(resolved.chain, ["scripts/m/fixtures", "app/src/data/link"]);
        assert!(resolved.reads("app/src/data/real/x.sql"));
        assert!(
            resolved.reads("app/src/data/link"),
            "retargeting a hop changes the read"
        );
        assert!(!resolved.reads("app/src/other.rs"));
    }

    #[test]
    fn escaping_and_dangling_links_fail_closed() {
        let root = tempfile::tempdir().expect("root");
        let base = root.path();
        std::fs::create_dir_all(base.join("a")).expect("dirs");
        symlink("../../outside", base.join("a/up")).expect("link");
        symlink("/etc/hosts", base.join("a/abs")).expect("link");
        symlink("missing", base.join("a/gone")).expect("link");
        symlink("loop2", base.join("a/loop1")).expect("link");
        symlink("loop1", base.join("a/loop2")).expect("link");
        assert_eq!(
            resolve_link(base, "a/up"),
            Err("escapes_repository".to_owned())
        );
        assert_eq!(
            resolve_link(base, "a/abs"),
            Err("escapes_repository".to_owned())
        );
        assert_eq!(resolve_link(base, "a/gone"), Err("dangling".to_owned()));
        assert_eq!(resolve_link(base, "a/loop1"), Err("loop".to_owned()));
    }
}
