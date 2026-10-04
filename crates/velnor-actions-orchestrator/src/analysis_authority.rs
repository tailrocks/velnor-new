//! Live GitHub evidence is the only constructor for reusable Cargo analysis.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use velnor_actions_contract::{canonical_json_bytes, digest_b3, parse_strict_json};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, RuntimePaths, ToolCatalog};

use crate::analysis_inventory::{AnalysisIdentity, payload_identity};
use crate::cover::shard_baseline::BaselineLookup;
use crate::run_select::{SelectedBaseRun, select_exact_base_run};

#[path = "analysis_archive.rs"]
mod archive;

/// Capability cannot be forged by caller-provided/local JSON or request fields.
#[derive(Debug)]
pub(crate) struct RemoteAnalysisAuthority {
    identity: AnalysisIdentity,
    payload_sha256: String,
}

impl RemoteAnalysisAuthority {
    pub(crate) fn identity(&self) -> &AnalysisIdentity {
        &self.identity
    }

    /// Bind every payload byte, including records, to the authenticated download.
    pub(crate) fn authenticates(&self, text: &str) -> bool {
        self.payload_sha256 == sha256(text.as_bytes())
    }

    #[cfg(test)]
    pub(crate) fn fixture(identity: AnalysisIdentity, text: &str) -> Self {
        Self {
            identity,
            payload_sha256: sha256(text.as_bytes()),
        }
    }
}

/// Verified bytes and their unforgeable origin capability travel together.
pub(crate) struct AuthenticatedAnalysis {
    pub(crate) text: String,
    pub(crate) authority: RemoteAnalysisAuthority,
}

/// Lookup inputs are expectations; only remote responses can supply authority.
#[derive(Clone, Copy)]
pub(crate) struct AnalysisLookupInputs<'a> {
    pub(crate) catalog: &'a ToolCatalog,
    pub(crate) root: &'a Path,
    pub(crate) base: &'a str,
    pub(crate) workflow: &'a str,
    pub(crate) branch: &'a str,
    pub(crate) repository: Option<&'a str>,
    pub(crate) helper_sha256: &'a str,
}

/// Derive immutable artifact identity before Cargo is installed or invoked.
pub(crate) fn analysis_artifact_name(
    base: &str,
    helper_sha256: &str,
    cargo_pin: &str,
) -> Result<String, String> {
    if base.len() != 40 || !base.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("analysis_base_invalid".to_owned());
    }
    if !valid_sha256(helper_sha256) || cargo_pin != ToolCatalog::pinned().rustup_toolchain() {
        return Err("analysis_compat_invalid".to_owned());
    }
    let bytes = canonical_json_bytes(&serde_json::json!({
        "schema": crate::analysis_inventory::ANALYSIS_INVENTORY_SCHEMA,
        "helper_sha256": helper_sha256,
        "cargo_pin": cargo_pin,
    }))
    .map_err(|_| "analysis_compat_invalid".to_owned())?;
    Ok(format!("velnor-analysis-{base}-{}", digest_b3(&bytes)))
}

/// Retrieve exact-base successful push analysis through authenticated pinned gh.
/// All failures are misses; caller must return NeedsCargo or perform fresh Cargo.
pub(crate) fn retrieve_analysis(
    inputs: AnalysisLookupInputs<'_>,
) -> Result<AuthenticatedAnalysis, String> {
    let helper = crate::cover_identity::generator::current_exe_sha256()
        .ok_or_else(|| "analysis_helper_unverifiable".to_owned())?;
    if helper != inputs.helper_sha256 {
        return Err("analysis_helper_mismatch".to_owned());
    }
    let lookup = lookup(inputs)?;
    let name = analysis_artifact_name(
        inputs.base,
        inputs.helper_sha256,
        &inputs.catalog.rustup_toolchain(),
    )?;
    let text = BaselineLookup::run_in_runtime(
        inputs.catalog,
        inputs.root,
        lookup.list_args(),
        RuntimePaths::planning(),
    )?;
    let selected = select_exact_base_run(&text, inputs.base, inputs.branch)?;
    verify_run(inputs, &lookup, selected)?;
    let listed = BaselineLookup::run_in_runtime(
        inputs.catalog,
        inputs.root,
        lookup.artifacts_args(selected.run_id),
        RuntimePaths::planning(),
    )?;
    let artifact = select_artifact(&listed, &name, &lookup, selected)?;
    let bytes = download_archive(inputs, &lookup, artifact.id)?;
    if sha256(&bytes) != artifact.sha256 {
        return Err("analysis_archive_digest_mismatch".to_owned());
    }
    let text = archive::payload(&bytes)?;
    let identity = payload_identity(&text)?;
    verify_identity(inputs, &lookup, selected, &identity)?;
    // Re-read live attempt evidence: a rerun racing retrieval cannot authorize
    // bytes from a now superseded attempt.
    verify_run(inputs, &lookup, selected)?;
    let authority = RemoteAnalysisAuthority {
        identity,
        payload_sha256: sha256(text.as_bytes()),
    };
    Ok(AuthenticatedAnalysis { text, authority })
}

fn lookup(inputs: AnalysisLookupInputs<'_>) -> Result<BaselineLookup, String> {
    let origin = crate::cover_baseline::provenance_check::repository_slug_from_origin(inputs.root);
    let expected = crate::cover_baseline::provenance_resolve::resolve_expected_repository(
        origin.as_deref(),
        inputs.repository,
    );
    if expected.conflict {
        return Err("analysis_repository_conflict".to_owned());
    }
    let repository = expected
        .slug
        .ok_or_else(|| "analysis_repository_missing".to_owned())?;
    let lookup = BaselineLookup::new(inputs.base, inputs.workflow, inputs.branch, &repository)?;
    let repo = api_json(inputs, format!("repos/{repository}"))?;
    if !repository_matches(repo["full_name"].as_str(), &repository)
        || repo["default_branch"] != inputs.branch
    {
        return Err("analysis_default_branch_mismatch".to_owned());
    }
    Ok(lookup)
}

fn verify_run(
    inputs: AnalysisLookupInputs<'_>,
    lookup: &BaselineLookup,
    selected: SelectedBaseRun,
) -> Result<(), String> {
    let run = api_json(
        inputs,
        format!("repos/{}/actions/runs/{}", lookup.repo, selected.run_id),
    )?;
    verify_run_value(&run, inputs, lookup, selected)
}

fn verify_run_value(
    run: &serde_json::Value,
    inputs: AnalysisLookupInputs<'_>,
    lookup: &BaselineLookup,
    selected: SelectedBaseRun,
) -> Result<(), String> {
    if run["id"].as_u64() != Some(selected.run_id)
        || run["run_attempt"].as_u64() != Some(selected.attempt)
        || run["head_sha"] != inputs.base
        || run["head_branch"] != inputs.branch
        || run["event"] != "push"
        || run["status"] != "completed"
        || run["conclusion"] != "success"
        || run["path"] != inputs.workflow
        || !repository_matches(run["repository"]["full_name"].as_str(), &lookup.repo)
        || !repository_matches(run["head_repository"]["full_name"].as_str(), &lookup.repo)
    {
        return Err("analysis_run_mismatch".to_owned());
    }
    Ok(())
}

struct SelectedArtifact {
    id: u64,
    sha256: String,
}

fn select_artifact(
    text: &str,
    name: &str,
    lookup: &BaselineLookup,
    selected: SelectedBaseRun,
) -> Result<SelectedArtifact, String> {
    let value = parse_strict_json(text).map_err(|_| "analysis_artifact_invalid".to_owned())?;
    let entries = value["artifacts"]
        .as_array()
        .ok_or_else(|| "analysis_artifact_invalid".to_owned())?;
    let mut matching = entries.iter().filter(|entry| entry["name"] == name);
    let entry = matching
        .next()
        .ok_or_else(|| "analysis_artifact_missing".to_owned())?;
    if matching.next().is_some()
        || entry["expired"].as_bool() != Some(false)
        || entry["workflow_run"]["id"].as_u64() != Some(selected.run_id)
        || entry["workflow_run"]["head_sha"] != lookup.base_sha
        || entry["workflow_run"]["head_branch"] != lookup.branch
        || entry["size_in_bytes"]
            .as_u64()
            .is_none_or(|size| size == 0 || size > 8 * 1024 * 1024)
    {
        return Err("analysis_artifact_mismatch".to_owned());
    }
    let id = entry["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| "analysis_artifact_invalid".to_owned())?;
    let digest = entry["digest"]
        .as_str()
        .and_then(|value| value.strip_prefix("sha256:"))
        .filter(|digest| valid_sha256(digest))
        .ok_or_else(|| "analysis_artifact_digest_missing".to_owned())?;
    Ok(SelectedArtifact {
        id,
        sha256: digest.to_ascii_lowercase(),
    })
}

fn verify_identity(
    inputs: AnalysisLookupInputs<'_>,
    lookup: &BaselineLookup,
    selected: SelectedBaseRun,
    identity: &AnalysisIdentity,
) -> Result<(), String> {
    let source = &identity.source;
    if identity.helper_sha256 != inputs.helper_sha256
        || identity.cargo_pin != inputs.catalog.rustup_toolchain()
        || !repository_matches(Some(&source.repository), &lookup.repo)
        || source.head_sha != inputs.base
        || source.workflow_sha != inputs.base
        || source.run_id != selected.run_id
        || u64::from(source.run_attempt) != selected.attempt
        || source.branch != inputs.branch
    {
        return Err("analysis_identity_mismatch".to_owned());
    }
    Ok(())
}

fn api_json(
    inputs: AnalysisLookupInputs<'_>,
    endpoint: String,
) -> Result<serde_json::Value, String> {
    let text = BaselineLookup::run_in_runtime(
        inputs.catalog,
        inputs.root,
        vec![OsString::from("api"), OsString::from(endpoint)],
        RuntimePaths::planning(),
    )?;
    parse_strict_json(&text).map_err(|_| "analysis_api_invalid".to_owned())
}

fn download_archive(
    inputs: AnalysisLookupInputs<'_>,
    lookup: &BaselineLookup,
    artifact_id: u64,
) -> Result<Vec<u8>, String> {
    let args = vec![
        OsString::from("api"),
        OsString::from(format!(
            "repos/{}/actions/artifacts/{artifact_id}/zip",
            lookup.repo,
        )),
    ];
    let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
        .map_err(|err| err.to_string())?;
    let output = exec
        .command_with_runtime(inputs.catalog, RuntimePaths::planning())
        .map_err(|err| err.to_string())?
        .with_cwd(inputs.root.to_path_buf())
        .run()
        .map_err(|err| err.to_string())?;
    if !output.success || output.stdout.len() > archive::MAX_ARCHIVE_BYTES {
        return Err("analysis_archive_unavailable".to_owned());
    }
    Ok(output.stdout)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !value.bytes().all(|byte| byte == b'0')
}

/// GitHub repository names are case insensitive; use the origin's one validator.
fn repository_matches(repository: Option<&str>, expected: &str) -> bool {
    repository
        .and_then(crate::origin::validate_repository_slug)
        .is_some_and(|repository| repository == expected)
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "analysis_authority_tests.rs"]
mod tests;
