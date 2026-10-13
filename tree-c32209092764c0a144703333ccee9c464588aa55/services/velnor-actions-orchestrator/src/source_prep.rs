//! P08 Cargo source preparation: shared snapshot, offline skip, single writer.
//!
//! Gate 1 orders resolution after preparation, but P08 restores the shared
//! sources snapshot and configures MBX BEFORE any fetch: when the metadata
//! probe proves all locked sources present, the step skips online fetch and
//! obligations run `--offline`; a cold/incomplete cache fetches through this
//! explicit path and records the miss in the job log. Steps are gated on
//! lockfile presence (lockless emits nothing). The plan job is the single
//! race-safe trusted writer (owned homes, saves once); crate jobs restore
//! read-only and never save the shared key.

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::{Step, StepRole};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::OrchestratorError;
use crate::discover::{PlannedWorkspace, workspace_lock, workspace_manifest};

/// Display name of the root-workspace fetch step.
pub(crate) const FETCH_SOURCES_STEP: &str = "Fetch Cargo sources";

/// Runner-owned scratch dir outside any checkout for ambient cargo.
///
/// Cargo discovers `.cargo/config.toml` from the working directory
/// upward, never from `--manifest-path` (proven: a clean-cwd fetch
/// ignores repo config and uses default sources). Ambient cargo runs
/// here so repo credential-providers never execute with ambient auth.
pub(crate) const CARGO_CLEAN_DIR: &str = "$RUNNER_TEMP/velnor/cargo-clean";

/// Enter the cargo isolation dir, creating it first (script prefix).
///
/// Every ambient cargo script starts with this; the dir is runner-
/// owned and outside the checkout, so no ancestor carries repo config.
pub(crate) fn cargo_isolation_prefix() -> String {
    format!("mkdir -p \"{CARGO_CLEAN_DIR}\" && cd \"{CARGO_CLEAN_DIR}\" && ")
}

/// Absolute `--manifest-path` flag for one validated fetch root.
///
/// The manifest anchors on `$GITHUB_WORKSPACE` (the runner-owned
/// checkout root); roots pass [`validate_root`], which rejects every
/// character unsafe inside double quotes.
pub(crate) fn isolated_manifest_flag(root: &str) -> String {
    format!(
        "--manifest-path \"$GITHUB_WORKSPACE/{}\"",
        workspace_manifest(root)
    )
}

/// Privilege-dropping cargo script over fixed parts (deny template).
///
/// `{ <install> && <prelude> } && <isolation> <payload>`: the ambient
/// install warms the pinned tool (no repo code can run: the install
/// carries `--no-config`, so mise loads no repo config to read hooks,
/// tasks, or plugins from; the step env stays ambient for
/// authenticated quota, so the flag alone bars repo config), the
/// shared credential-unset prelude removes every
/// ambient secret, and only then does the payload run from the cargo
/// isolation dir. Both parts are fixed generator values, never
/// repository shell; the brace group fails closed on install failure
/// instead of degrading to an ambient payload. This is the second
/// fixed `sh` template in this module (fetch probe-and-fetch is the
/// first): shell sequencing for cargo isolation lives here, so the
/// orchestrator's `sh` confinement set stays closed.
pub(crate) fn privilege_drop_argv(install: &str, payload: &str) -> Vec<String> {
    let prelude = velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude();
    let script = format!(
        "{{ {install} && {prelude} }} && {}{payload}",
        cargo_isolation_prefix(),
    );
    vec!["sh".to_owned(), "-c".to_owned(), script]
}

/// Sorted workspace roots whose lockfile exists under `root`.
///
/// Mirrors the gating in `inventory::qualify_workspaces` through the
/// shared [`workspace_lock`] helper, so fetch and qualification never
/// disagree about which workspaces are pinned.
pub(crate) fn lockful_roots(root: &Path, workspaces: &[PlannedWorkspace]) -> Vec<String> {
    let mut roots = Vec::new();
    for workspace in workspaces {
        let lock = workspace_lock(&workspace.record.workspace_root);
        if root.join(&lock).is_file() {
            roots.push(workspace.record.workspace_root.clone());
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

/// Fetch steps for crate jobs (readers): probe, skip when warm, else fetch.
///
/// Env is the validated obligation contract, so fetch and consumer match
/// by construction. Rust-only: callers emit no fetch for pure-tofu roles.
/// # Errors
///
/// Returns contract errors for rejected roots or step-env failures.
pub(crate) fn fetch_steps_for_crate(
    catalog: &ToolCatalog,
    roots: &[String],
) -> Result<Vec<Step>, OrchestratorError> {
    let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    fetch_steps_with(catalog, roots, &env)
}

/// Fetch steps for the plan job (trusted writer): same owned homes.
///
/// The writer seeds the shared snapshot at the same Cargo-home expression
/// readers restore, so save and restore never disagree on location.
/// # Errors
///
/// Returns contract errors for rejected roots or step-env failures.
pub(crate) fn fetch_steps_for_plan(
    catalog: &ToolCatalog,
    roots: &[String],
) -> Result<Vec<Step>, OrchestratorError> {
    let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    fetch_steps_with(catalog, roots, &env)
}

/// One probe-and-fetch `sh -c` step per lockful root.
///
/// The script runs `cargo metadata --locked --offline` first: success
/// skips online fetch (warm path); failure fetches explicitly and echoes
/// the miss reason. Nested roots name their manifest in argv and step name.
/// # Errors
///
/// Returns contract errors for unsafe roots or rejected vectors.
fn fetch_steps_with(
    catalog: &ToolCatalog,
    roots: &[String],
    env: &BTreeMap<String, String>,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::with_capacity(roots.len());
    for root in roots {
        validate_root(root)?;
        let script = fetch_script(catalog, root);
        let manifest = workspace_manifest(root);
        let name = if root.is_empty() {
            FETCH_SOURCES_STEP.to_owned()
        } else {
            format!("{FETCH_SOURCES_STEP} ({manifest})")
        };
        let mut step = velnor_actions_workflow_renderer::ambient_shell_step(
            &name,
            vec!["sh".to_owned(), "-c".to_owned(), script],
            env.clone(),
        )
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
        step.role = Some(StepRole::CargoSourcesFetch);
        steps.push(step);
    }
    Ok(steps)
}

/// Fixed probe-and-fetch script for one root (manifest quoted, no injection).
///
/// Runs from the cargo isolation dir with an absolute manifest path,
/// so repo `.cargo/config.toml` (credential-providers, source
/// replacement) is never read while ambient auth is in scope.
fn fetch_script(catalog: &ToolCatalog, root: &str) -> String {
    let spec = catalog.tool_spec(PinnedTool::Rust);
    let base = format!("mise --no-config --no-env --no-hooks exec {spec} -- cargo");
    let manifest = format!(" {}", isolated_manifest_flag(root));
    let probe = format!("{base} metadata --locked --offline{manifest} >/dev/null 2>&1");
    let fetch = format!("{base} fetch --locked{manifest}");
    format!(
        "{}if {probe}; then echo \"velnor: sources hit, skipping fetch\"; else echo \"velnor: sources miss (source_missing), fetching\"; {fetch}; fi",
        cargo_isolation_prefix()
    )
}

/// Reject roots unsafe for shell interpolation or cache keys.
///
/// Thin error mapping over the contract single source (shared with the
/// renderer's ambient-auth exemption, so both sides agree by
/// construction instead of mirroring the predicate).
pub(crate) fn validate_root(root: &str) -> Result<(), OrchestratorError> {
    velnor_actions_contract::validate_fetch_root(root).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
}
#[cfg(test)]
mod tests;
