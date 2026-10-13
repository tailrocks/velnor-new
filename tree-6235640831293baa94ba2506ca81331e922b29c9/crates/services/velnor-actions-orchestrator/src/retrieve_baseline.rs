//! Event-time baseline fetch: exact download into `<run-dir>/baseline.json`.
//!
//! The plan artifact stages `baseline.json` only when the planner
//! classified from a request-supplied manifest; production plans
//! resolve coverage through a live lookup whose manifest never rides
//! the artifact. The fetch step closes that gap: when the downloaded
//! plan carries covered obligations and no staged `baseline.json`, it
//! re-derives the exact artifact name from the plan base plus
//! plan-derived compatibility and downloads that one exact artifact,
//! bounded (a whole-run fallback does not exist). Staged evidence
//! always wins: an existing `baseline.json` — file or symlink —
//! skips the download, and the final write is exclusive, never an
//! overwrite. Every miss returns `false` so the merge judges
//! `source_missing` itself; the fetch step still exits success.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::{Plan, canonical_json_bytes};
use velnor_actions_mise::ToolCatalog;

use crate::cover::shard::{BaselineLookup, resolve_manifests};

/// Fetch the plan's exact baseline into `<run-dir>/baseline.json`.
///
/// Strict-typed plan parse, covered-obligation gate, staged-wins
/// short-circuit, then default-branch resolution plus the exact
/// download. `repo` is the already-validated repository slug scoping
/// every lookup call. `true` only when these bytes were written here.
pub(crate) fn retrieve_baseline_to(
    catalog: &ToolCatalog,
    run_dir: &Path,
    plan: &serde_json::Value,
    repo: &str,
) -> bool {
    let Ok(typed) = serde_json::from_value::<Plan>(plan.clone()) else {
        return false;
    };
    let Some(base) = typed.base.as_deref() else {
        return false;
    };
    if !plan_has_covered(&typed) {
        return false;
    }
    if staged_baseline_present(run_dir) {
        return false;
    }
    let Ok(name) = crate::cover_baseline::lookup_artifact_name(&typed, base) else {
        return false;
    };
    let Some(branch) = default_branch_for(catalog, run_dir, repo) else {
        return false;
    };
    let workflow = velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
    let found = resolve_manifests(
        catalog,
        run_dir,
        base,
        workflow,
        &branch,
        Some(&name),
        Some(repo),
    );
    let Ok(found) = found else {
        return false;
    };
    let Some(first) = found.into_iter().next() else {
        return false;
    };
    stage_manifest(run_dir, &first)
}

/// True when any obligation claims trusted-baseline coverage.
///
/// Delegates to the single covered-predicate so staging, fetching,
/// encoding, and the skip gate can never disagree on what counts.
fn plan_has_covered(plan: &Plan) -> bool {
    crate::covered_tasks::plan_has_covered(plan)
}

/// True when staged evidence already answers the merge.
///
/// `symlink_metadata` never follows: a planted symlink counts as
/// present (skip) so the fetch never writes through it.
fn staged_baseline_present(run_dir: &Path) -> bool {
    std::fs::symlink_metadata(run_dir.join(crate::baseline_publish::BASELINE_FILENAME)).is_ok()
}

/// Default branch for the explicit repository through pinned `gh`.
///
/// The final job has no checkout, so config and origin cannot name
/// the branch the protected push published on; the repository itself
/// is the authority. Empty argv (bad slug) and any lookup failure
/// miss before the run listing.
fn default_branch_for(catalog: &ToolCatalog, cwd: &Path, repo: &str) -> Option<String> {
    let args = default_branch_args(repo);
    if args.is_empty() {
        return None;
    }
    let text = BaselineLookup::run(catalog, cwd, args).ok()?;
    parse_default_branch(&text)
}

/// Fixed `gh api` argv reading one repository's default branch.
///
/// The path names the expected repository explicitly: no
/// `{owner}`/`{repo}` template ever resolves from ambient state. A
/// malformed slug yields no command instead of an unscoped one.
fn default_branch_args(repo: &str) -> Vec<OsString> {
    let Some(repo) = crate::origin::validate_repository_slug(repo) else {
        return Vec::new();
    };
    ["api", &format!("repos/{repo}"), "--jq", ".default_branch"]
        .iter()
        .map(OsString::from)
        .collect()
}

/// Strict default-branch name from one `gh api` response.
///
/// Accepts the raw name or its JSON-quoted form; anything else —
/// empty, whitespace, traversal, HEAD, lookup-hostile punctuation —
/// misses so a hostile response never becomes a `--branch` filter.
fn parse_default_branch(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(trimmed);
    let branch = unquoted.trim();
    if branch.is_empty()
        || branch == "HEAD"
        || branch.contains("..")
        || branch.chars().any(char::is_whitespace)
        || ["://", "*", "$", ";", " ", "\""]
            .iter()
            .any(|token| branch.contains(token))
    {
        return None;
    }
    Some(branch.to_owned())
}

/// Canonically stage one resolved manifest as `baseline.json`.
///
/// Exclusive create: staged evidence is never overwritten, and a
/// symlink planted between the presence check and this write refuses
/// instead of writing through.
fn stage_manifest(run_dir: &Path, manifest: &crate::merge::BaselineManifest) -> bool {
    let target = run_dir.join(crate::baseline_publish::BASELINE_FILENAME);
    let Ok(bytes) = canonical_json_bytes(manifest) else {
        return false;
    };
    crate::exclusive_write::write_exclusive(&target, &bytes, "baseline_stage").is_ok()
}

#[cfg(test)]
mod tests;
