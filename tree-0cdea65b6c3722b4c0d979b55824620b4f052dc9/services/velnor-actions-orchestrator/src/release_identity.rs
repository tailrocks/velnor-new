//! Release identity derivation: repository, source, registry, lock slug.
//!
//! The config schema carries no repository, routine source SHA, plan id,
//! or registry inputs, so emission derives them deterministically: the
//! repository from the local `origin` URL (the sole authority; no
//! environment hint can unlock or widen it), the routine source from
//! `HEAD`, the plan id from the source SHA, the registry from the
//! selected set's intersection, and the lock slug from the release
//! manifest directory. Every miss fails closed.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::GitRequest;
use velnor_actions_rust_core::release_select::ReleaseSelection;
use velnor_actions_workflow_release::release_spec::validate_source_sha;

use crate::OrchestratorError;
use crate::origin::origin_url_via_git;

/// Derive `owner/repo` from the local `origin` URL.
///
/// Origin is the only authority: a missing or non-GitHub origin fails
/// closed instead of guessing from configuration or environment.
///
/// # Errors
///
/// Returns identity errors when no origin exists or it is not a
/// `github.com` repository URL.
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

/// Normalize an origin URL to `owner/repo` (case preserved).
///
/// Accepts `https:`/`ssh:` URLs, scp-like `host:path`, and bare
/// `owner/repo`; anything else (including non-GitHub hosts) yields `None`.
/// Case is preserved: the publish gate compares against the live
/// `github.repository` value, which keeps its registered casing.
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

/// True for one `owner/repo` pair over repository segments.
fn is_owner_repo(path: &str) -> bool {
    let Some((owner, name)) = path.split_once('/') else {
        return false;
    };
    !name.contains('/') && is_repo_segment(owner) && is_repo_segment(name)
}

/// True for one `owner`/`repo` segment over `[A-Za-z0-9_.-]`.
fn is_repo_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 100
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Resolve `HEAD` to its full SHA for the routine (OIDC) source.
///
/// The tree state under generation is the exact source; dispatch-time
/// inputs can only reaffirm it, never widen it.
///
/// # Errors
///
/// Returns contract errors when Git cannot run or the tree has no commit.
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

/// Derive the plan id from the approved source SHA.
///
/// `release-<12 hex>` is stable for one source, distinct across
/// sources, and fits the plan-id charset and length bound.
pub(crate) fn plan_id_for_source(sha: &str) -> String {
    format!("release-{}", &sha[..12])
}

/// Intersect selected registries; the first sorted survivor wins.
///
/// # Errors
///
/// Returns a contract error when the selection spans registries with
/// no common member: the approved plan carries exactly one.
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

/// Slugify the release manifest directory for the publisher lock.
///
/// `Cargo.toml` maps to `root`; anything else lowercases to
/// `[a-z0-9_-]`, capped at 64 characters.
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
#[cfg(test)]
mod tests;
