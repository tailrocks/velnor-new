use std::path::{Component, Path, PathBuf};

use crate::OrchestratorError;

const TOOL_FILES: [&str; 6] = [
    "mise.toml",
    ".mise.toml",
    "mise.lock",
    ".mise.lock",
    ".mise-version",
    "rust-toolchain.toml",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSnapshot {
    entries: Vec<(String, Option<Vec<u8>>)>,
    unreadable: Vec<String>,
}

impl ToolSnapshot {
    #[must_use]
    pub fn capture(root: &Path) -> Self {
        let mut unreadable = Vec::new();
        let entries = TOOL_FILES
            .iter()
            .map(|rel| {
                let bytes = match std::fs::read(root.join(rel)) {
                    Ok(bytes) => Some(bytes),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                    Err(_) => {
                        unreadable.push((*rel).to_owned());
                        None
                    }
                };
                ((*rel).to_owned(), bytes)
            })
            .collect();
        Self {
            entries,
            unreadable,
        }
    }

    pub fn verify(&self, root: &Path) -> Result<(), OrchestratorError> {
        let fresh = Self::capture(root);
        let mut bad = self.unreadable.clone();
        bad.extend(fresh.unreadable.iter().cloned());
        if let Some(first) = bad.iter().min() {
            return Err(OrchestratorError::Contract {
                problem: format!("tool_files_unreadable:{first}"),
            });
        }
        for ((rel, want), (_, got)) in self.entries.iter().zip(fresh.entries.iter()) {
            if want != got {
                return Err(OrchestratorError::Contract {
                    problem: format!("tool_files_changed:{rel}"),
                });
            }
        }
        Ok(())
    }
}

const GENERATE_LOCK_NAME: &str = ".github.velnor-generate.lock";

#[derive(Debug)]
pub(crate) struct GenerateOwnership {
    path: PathBuf,
}

impl GenerateOwnership {
    pub(crate) fn acquire(root: &Path) -> Result<Self, OrchestratorError> {
        let path = root.join(GENERATE_LOCK_NAME);
        match std::fs::create_dir(&path) {
            Ok(()) => Ok(Self { path }),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(OrchestratorError::Contract {
                    problem: format!("concurrent_generate:{}", path.display()),
                })
            }
            Err(err) => Err(OrchestratorError::io(
                path.display().to_string(),
                err.to_string(),
            )),
        }
    }
}

impl Drop for GenerateOwnership {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir(&self.path);
    }
}

fn preview_refused(path: &str, reason: &str) -> OrchestratorError {
    OrchestratorError::PreviewRefused {
        path: path.to_owned(),
        reason: reason.to_owned(),
    }
}

pub(crate) fn prepare_preview_dir(root: &Path, dest: &Path) -> Result<PathBuf, OrchestratorError> {
    let label = dest.display().to_string();
    let root_canon = root
        .canonicalize()
        .map_err(|err| OrchestratorError::io(label.clone(), err.to_string()))?;
    if let Ok(meta) = std::fs::symlink_metadata(dest) {
        if meta.is_symlink() {
            return Err(preview_refused(&label, "symlink_refused"));
        }
        if !meta.is_dir() {
            return Err(preview_refused(&label, "not_a_directory"));
        }
        let mut entries = std::fs::read_dir(dest)
            .map_err(|err| OrchestratorError::io(label.clone(), err.to_string()))?;
        if entries.next().is_some() {
            return Err(preview_refused(&label, "non_empty"));
        }
    }
    let (anchor, suffix) = preview_anchor(dest, &label)?;
    refuse_inside_or_ancestor(&root_canon, &anchor.join(&suffix), &label)?;
    create_preview_chain(&anchor, &suffix, &label)?;
    let canonical = dest
        .canonicalize()
        .map_err(|err| OrchestratorError::io(label.clone(), err.to_string()))?;
    refuse_inside_or_ancestor(&root_canon, &canonical, &label)?;
    reserve_preview_github(&canonical, &label)?;
    Ok(canonical)
}

fn preview_anchor(dest: &Path, label: &str) -> Result<(PathBuf, PathBuf), OrchestratorError> {
    let mut found = None;
    for candidate in dest.ancestors() {
        match std::fs::symlink_metadata(candidate) {
            Ok(meta) if meta.is_symlink() => {
                return Err(preview_refused(label, "symlink_refused"));
            }
            Ok(_) => {
                found = Some(candidate);
                break;
            }
            Err(_) => {}
        }
    }
    let Some(anchor) = found else {
        return Err(OrchestratorError::io(
            label.to_owned(),
            "no_existing_ancestor",
        ));
    };
    let canonical = anchor
        .canonicalize()
        .map_err(|err| OrchestratorError::io(label.to_owned(), err.to_string()))?;
    let mut suffix = PathBuf::new();
    let rest = dest
        .strip_prefix(anchor)
        .map_err(|_| preview_refused(label, "traversal"))?;
    for component in rest.components() {
        match component {
            Component::Normal(part) => suffix.push(part),
            Component::CurDir => {}
            _ => return Err(preview_refused(label, "traversal")),
        }
    }
    Ok((canonical, suffix))
}

fn refuse_inside_or_ancestor(
    root: &Path,
    candidate: &Path,
    label: &str,
) -> Result<(), OrchestratorError> {
    if candidate == root || candidate.starts_with(root) {
        return Err(preview_refused(label, "inside_repository"));
    }
    if root.starts_with(candidate) {
        return Err(preview_refused(label, "ancestor_of_repository"));
    }
    Ok(())
}

fn create_preview_chain(
    anchor: &Path,
    suffix: &Path,
    label: &str,
) -> Result<(), OrchestratorError> {
    let mut current = anchor.to_path_buf();
    for component in suffix.components() {
        current.push(component);
        drop(std::fs::create_dir(&current));
        let meta = std::fs::symlink_metadata(&current)
            .map_err(|err| OrchestratorError::io(label.to_owned(), err.to_string()))?;
        if meta.is_symlink() {
            return Err(preview_refused(label, "symlink_refused"));
        }
        if !meta.is_dir() {
            return Err(preview_refused(label, "not_a_directory"));
        }
    }
    Ok(())
}

fn reserve_preview_github(canonical: &Path, label: &str) -> Result<(), OrchestratorError> {
    let github = canonical.join(".github");
    match std::fs::create_dir(&github) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(preview_refused(label, "concurrent_preview"));
        }
        Err(err) => {
            return Err(OrchestratorError::io(label.to_owned(), err.to_string()));
        }
    }
    let entries = std::fs::read_dir(canonical)
        .map_err(|err| OrchestratorError::io(label.to_owned(), err.to_string()))?;
    let raced = entries
        .filter_map(Result::ok)
        .any(|entry| entry.file_name() != ".github");
    if raced {
        let _unreserved = std::fs::remove_dir(&github);
        return Err(preview_refused(label, "concurrent_preview"));
    }
    Ok(())
}
