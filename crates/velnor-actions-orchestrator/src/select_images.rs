// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::config::{DockerfileFacts, analyze_dockerfile};
use velnor_actions_contract::{
    ImageConfig, ImagePlatform, ImagePrebuilt, JobDisposition, PlanJob, WorkflowEvent, selects_affected_work,
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
                    .any(|claim| path == claim
                        || path.starts_with(&format!("{claim}/"))
                        || claim.starts_with(&format!("{path}/")))
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
    include_runner_family: bool,
) -> Vec<PlanJob> {
    let mut jobs = Vec::new();
    for image in images {
        for platform in &image.platforms {
            let selected = match (selects_affected_work(event), affected, &evidence.paths) {
                (true, Some(packages), Some(changed)) => {
                    image_selected(root, image, *platform, discovery, packages, changed)
                }
                _ => true,
            };
            let disposition = if selected {
                JobDisposition::Run
            } else {
                JobDisposition::NotAffected
            };
            jobs.push(PlanJob {
                lane: None,
                job_id: image.build_job_id(*platform),
                disposition,
            });
        }
    }
    if include_runner_family {
        jobs.push(runner_family_disposition(
            root, discovery, event, affected, evidence,
        ));
    }
    jobs
}

fn image_selected(
    root: &Path,
    image: &ImageConfig,
    platform: ImagePlatform,
    discovery: &Discovery,
    packages: &BTreeSet<String>,
    changed: &BTreeSet<String>,
) -> bool {
    let Some(claims) = claims(root, image, platform, changed) else {
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

fn claims(
    root: &Path,
    image: &ImageConfig,
    platform: ImagePlatform,
    changed: &BTreeSet<String>,
) -> Option<Claims> {
    let mut claims = Claims::default();
    let context = image.context.as_str();
    let under = |relative: &str| {
        if relative == "." {
            context.to_owned()
        } else if context == "." {
            relative.to_owned()
        } else {
            format!("{context}/{relative}")
        }
    };
    claims.claim(&under(".dockerignore"));
    {
        let dockerfile = image.dockerfile.for_platform(platform)?;
        claims.claim(dockerfile);
        claims.claim(&format!("{dockerfile}.dockerignore"));
        let Ok(RepoRead::Text(text)) = read_repo_file(root, dockerfile, MAX_DOCKERFILE_BYTES)
        else {
            return None;
        };
        let facts = analyze_dockerfile(&text, &[]).ok()?;
        if facts.claims_whole_context || text.contains("--mount") {
            claims.claim(context);
        }
        claim_context_sources(&mut claims, &facts, context, changed);
    }
    Some(claims)
}

fn claim_context_sources(
    claims: &mut Claims,
    facts: &DockerfileFacts,
    context: &str,
    changed: &BTreeSet<String>,
) {
    for source in &facts.context_sources {
        let root = source_root(source);
        let claim = if context == "." {
            root.to_owned()
        } else if root.is_empty() || root == "." {
            context.to_owned()
        } else {
            format!("{context}/{root}")
        };
        if changed.iter().any(|path| {
            let relative = if context == "." {
                Some(path.as_str())
            } else {
                path.strip_prefix(context).and_then(|path| path.strip_prefix('/'))
            };
            relative.is_some_and(|path| {
                (root.is_empty() || root == "." || path == root || path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/')))
                    && !facts.source_excludes_path(source, path)
                    || root.strip_prefix(path).is_some_and(|rest| rest.starts_with('/'))
            })
        }) {
            claims.claim(&claim);
        }
    }
}

fn source_root(source: &str) -> &str {
    let source = source.trim_end_matches('/');
    match source.find(['*', '?', '[']) {
        Some(at) => source[..at].rsplit_once('/').map_or("", |(dir, _)| dir),
        None => source,
    }
}

fn runner_family_disposition(
    root: &Path,
    discovery: &Discovery,
    event: WorkflowEvent,
    affected: Option<&BTreeSet<String>>,
    evidence: &ChangeEvidence,
) -> PlanJob {
    let selected = match (selects_affected_work(event), affected, &evidence.paths) {
        (true, Some(packages), Some(changed)) => {
            runner_family_claims(root, changed).is_none_or(|claims| claims.touched_by(changed))
                || runner_family_prebuilt_selected(discovery, packages)
        }
        _ => true,
    };
    PlanJob {
        lane: None,
        job_id: velnor_actions_contract::config::RUNNER_IMAGE_FAMILY_JOB_ID.to_owned(),
        disposition: if selected {
            JobDisposition::Run
        } else {
            JobDisposition::NotAffected
        },
    }
}

fn runner_family_prebuilt_selected(discovery: &Discovery, packages: &BTreeSet<String>) -> bool {
    let names = [
        "velnor-resource-probe",
        "velnor-runner-host",
        "velnor-actions-cli",
    ];
    names.iter().any(|name| {
        let mut ids = discovery
            .workspaces
            .iter()
            .flat_map(|planned| planned.record.packages.iter())
            .filter(|package| package.in_workspace && !package.external && package.name == *name)
            .map(|package| package.id.as_str())
            .peekable();
        ids.peek().is_none() || ids.any(|id| packages.contains(id))
    })
}

fn runner_family_claims(root: &Path, changed: &BTreeSet<String>) -> Option<Claims> {
    let mut claims = Claims::default();
    for path in velnor_archive_guard::source_claims(root).ok()? {
        claims.claim(&path);
    }
    let inputs = velnor_actions_contract::runner_publication_source_inputs().ok()?;
    for &path in inputs {
        claims.claim(path);
    }
    for path in velnor_actions_workflow_renderer::verification_jobs::FAMILY_PREFETCH_SOURCES {
        claims.claim(path);
    }
    for path in [
        "scripts/check-runner-image-candidate.sh",
        ".cargo/config",
        ".cargo/config.toml",
        ".gitignore",
        ".mise-version",
        "mise.toml",
        "mise.lock",
        "rust-toolchain.toml",
        "Cargo.toml",
        "Cargo.lock",
        ".velnor/config.toml",
    ] {
        claims.claim(path);
    }
    for (role, context, dockerfile) in [
        (
            "runner",
            "images/runner/ubuntu-26.04",
            "images/runner/ubuntu-26.04/Dockerfile",
        ),
        ("dind", "images/dind", "images/dind/Dockerfile"),
        (
            "resource_probe",
            "images/resource-probe",
            "images/resource-probe/Dockerfile",
        ),
    ] {
        claims.claim(dockerfile);
        claims.claim(&format!("{context}/.dockerignore"));
        claims.claim(&format!("{dockerfile}.dockerignore"));
        let RepoRead::Text(text) = read_repo_file(root, dockerfile, MAX_DOCKERFILE_BYTES).ok()?
        else {
            return None;
        };
        let policy = velnor_actions_contract::runner_publication_dockerfile_context(role)?;
        let facts = policy.analyze(&text).ok()?;
        if facts.claims_whole_context || text.contains("--mount") {
            claims.claim(context);
        }
        claim_context_sources(&mut claims, &facts, context, changed);
    }
    Some(claims)
}
