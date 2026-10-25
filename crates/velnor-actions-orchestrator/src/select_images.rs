use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::config::analyze_dockerfile;
use velnor_actions_contract::{
    ImageConfig, ImagePrebuilt, JobDisposition, PlanJob, WorkflowEvent, selects_affected_work,
};

use crate::discover::Discovery;
use crate::safe_read::{RepoRead, read_repo_file};

const MAX_DOCKERFILE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ChangeEvidence {
    pub(crate) paths: Option<BTreeSet<String>>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Claims {
    everything: bool,
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

fn source_root(source: &str) -> &str {
    let source = source.trim_end_matches('/');
    match source.find(['*', '?', '[']) {
        Some(at) => source[..at].rsplit_once('/').map_or("", |(dir, _)| dir),
        None => source,
    }
}
