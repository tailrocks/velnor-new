//! Affected selection for container image build jobs (hard decision 3).
//!
//! Image jobs own no obligations, so selection plans them from what an image
//! build reads: its Dockerfile(s), the context paths its `COPY`/`ADD`
//! sources name (plus `.dockerignore` files), and, for a prebuilt
//! `cargo_bin`, the compiled package. Doubt selects the image: an unknown
//! change set, a global-config path, an unreadable Dockerfile, a Dockerfile
//! `RUN --mount` (which can read any context path) or an undiscovered
//! package all run the job. Full-run events (push, merge group,
//! qualification) never skip an image.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::config::analyze_dockerfile;
use velnor_actions_contract::{
    ImageConfig, ImagePrebuilt, JobDisposition, PlanJob, WorkflowEvent, selects_affected_work,
};

use crate::discover::Discovery;
use crate::safe_read::{RepoRead, read_repo_file};

const MAX_DOCKERFILE_BYTES: u64 = 1024 * 1024;

/// What a classification learned beyond the affected package set.
///
/// Image jobs depend on repository paths that no Rust package owns (a
/// Dockerfile, its context files), so selection hands them the exact change
/// set whenever the comparison was exact.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ChangeEvidence {
    /// Changed repository paths of an exact comparison. `None` means the
    /// change set is unknown or a global-config path changed: everything is
    /// affected.
    pub(crate) paths: Option<BTreeSet<String>>,
}

/// What one image build reads from the repository.
#[derive(Debug, Default, PartialEq, Eq)]
struct Claims {
    /// Every path of the repository (the context is the repository root).
    everything: bool,
    /// Repo-relative files and directories; a directory claims its subtree.
    paths: BTreeSet<String>,
}

impl Claims {
    fn claim(&mut self, path: &str) {
        let path = path.trim_matches('/');
        if path.is_empty() || path == "." {
            self.everything = true;
        } else {
            self.paths.insert(path.to_owned());
        }
    }

    fn touched_by(&self, changed: &BTreeSet<String>) -> bool {
        (self.everything && !changed.is_empty())
            || changed.iter().any(|path| {
                self.paths
                    .iter()
                    .any(|claim| path == claim || path.starts_with(&format!("{claim}/")))
            })
    }
}

/// Plan dispositions for every image build job, in declaration order.
///
/// `affected` is the classification's affected package set (`None` when the
/// comparison is unknown) and `evidence` carries the exact change set.
pub(crate) fn dispositions(
    root: &Path,
    images: &[ImageConfig],
    discovery: &Discovery,
    event: WorkflowEvent,
    affected: Option<&BTreeSet<String>>,
    evidence: &ChangeEvidence,
) -> Vec<PlanJob> {
    let mut jobs = Vec::new();
    for image in images {
        let selected = match (selects_affected_work(event), affected, &evidence.paths) {
            (true, Some(packages), Some(changed)) => {
                image_selected(root, image, discovery, packages, changed)
            }
            _ => true,
        };
        let disposition = if selected {
            JobDisposition::Run
        } else {
            JobDisposition::NotAffected
        };
        for platform in &image.platforms {
            jobs.push(PlanJob {
                job_id: image.build_job_id(*platform),
                disposition,
            });
        }
    }
    jobs
}

fn image_selected(
    root: &Path,
    image: &ImageConfig,
    discovery: &Discovery,
    packages: &BTreeSet<String>,
    changed: &BTreeSet<String>,
) -> bool {
    let Some(claims) = claims(root, image) else {
        return true;
    };
    claims.touched_by(changed) || prebuilt_selected(image, discovery, packages)
}

/// True when the compiled package may have changed, or cannot be located.
fn prebuilt_selected(
    image: &ImageConfig,
    discovery: &Discovery,
    packages: &BTreeSet<String>,
) -> bool {
    let Some(ImagePrebuilt::CargoBin { package, .. }) = &image.prebuilt else {
        return false;
    };
    let mut ids = discovery
        .workspaces
        .iter()
        .flat_map(|planned| planned.record.packages.iter())
        .filter(|p| p.in_workspace && !p.external && &p.name == package)
        .map(|p| p.id.as_str())
        .peekable();
    ids.peek().is_none() || ids.any(|id| packages.contains(id))
}

/// The repository paths an image build reads, or `None` when unknowable.
fn claims(root: &Path, image: &ImageConfig) -> Option<Claims> {
    let mut claims = Claims::default();
    let context = image.context.as_str();
    let under = |relative: &str| {
        if context == "." {
            relative.to_owned()
        } else {
            format!("{context}/{relative}")
        }
    };
    claims.claim(&under(".dockerignore"));
    for dockerfile in image.dockerfile.paths() {
        claims.claim(dockerfile);
        claims.claim(&format!("{dockerfile}.dockerignore"));
        let Ok(RepoRead::Text(text)) = read_repo_file(root, dockerfile, MAX_DOCKERFILE_BYTES)
        else {
            return None;
        };
        let facts = analyze_dockerfile(&text).ok()?;
        if text.contains("--mount") {
            claims.claim(context);
        }
        for source in &facts.context_sources {
            claims.claim(&under(source_root(source)));
        }
    }
    Some(claims)
}

/// The deepest literal directory prefix of a `COPY`/`ADD` source.
///
/// A glob claims the directory before its first wildcard; the context root
/// (`.`, empty) claims the whole context.
fn source_root(source: &str) -> &str {
    let source = source.trim_end_matches('/');
    match source.find(['*', '?', '[']) {
        Some(at) => source[..at].rsplit_once('/').map_or("", |(dir, _)| dir),
        None => source,
    }
}

#[cfg(test)]
#[path = "select_images_tests.rs"]
mod tests;
