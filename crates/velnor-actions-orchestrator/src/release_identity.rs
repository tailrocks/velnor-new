use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::GitRequest;
use velnor_actions_rust::release_select::ReleaseSelection;
use velnor_actions_workflow_renderer::release_spec::validate_source_sha;

use crate::OrchestratorError;
use crate::origin::origin_url_via_git;

pub(crate) fn origin_repository(root: &Path) -> Result<String, OrchestratorError> {
    let Some(url) = origin_url_via_git(root) else {
        return Err(OrchestratorError::IdentityRejected {
            problem: "release_origin_unresolvable:set remote origin to the repository URL"
                .to_owned(),
        });
    };
    normalize_origin(&url).ok_or_else(|| OrchestratorError::IdentityRejected {
        problem: "release_origin_not_github:origin must be a github.com owner/repo URL".to_owned(),
    })
}

fn normalize_origin(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let base = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    if let Some((host, path)) = crate::prepare::split_host_path(base) {
        if host.eq_ignore_ascii_case("github.com") && is_owner_repo(path) {
            return Some(path.to_owned());
        }
        return None;
    }
    is_owner_repo(base).then(|| base.to_owned())
}

fn is_owner_repo(path: &str) -> bool {
    let Some((owner, name)) = path.split_once('/') else {
        return false;
    };
    !name.contains('/') && is_repo_segment(owner) && is_repo_segment(name)
}

fn is_repo_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 100
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub(crate) fn head_sha(root: &Path) -> Result<String, OrchestratorError> {
    let output = GitRequest::rev_parse(vec![OsString::from("HEAD")])
        .run_in(root)
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    if !output.success {
        return Err(OrchestratorError::Contract {
            problem: "release_head_unresolvable:commit before generating release workflows"
                .to_owned(),
        });
    }
    let sha = output
        .stdout_text("git")
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    let sha = sha.trim().to_owned();
    validate_source_sha(&sha)?;
    Ok(sha)
}

pub(crate) fn plan_id_for_source(sha: &str) -> String {
    format!("release-{}", &sha[..12])
}

pub(crate) fn common_registry(selection: &ReleaseSelection) -> Result<String, OrchestratorError> {
    let mut common: Option<BTreeSet<&str>> = None;
    for package in &selection.packages {
        let registries: BTreeSet<&str> = package.registries.iter().map(String::as_str).collect();
        common = Some(match common {
            None => registries,
            Some(kept) => kept.intersection(&registries).copied().collect(),
        });
    }
    common
        .and_then(|kept| kept.into_iter().next().map(str::to_owned))
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "release_no_common_registry".to_owned(),
        })
}

pub(crate) fn workspace_slug(manifest_path: &str) -> String {
    let dir = manifest_path.rsplit_once('/').map_or("", |(head, _)| head);
    let mut slug: String = dir
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char.to_ascii_lowercase()
            } else if matches!(char, '-' | '_') {
                char
            } else {
                '-'
            }
        })
        .collect();
    if slug.len() > 64 {
        slug.truncate(64);
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "root".to_owned()
    } else {
        slug.to_owned()
    }
}
