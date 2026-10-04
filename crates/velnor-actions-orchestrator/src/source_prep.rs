//! P08 Cargo source preparation: locked offline probe and explicit fetch.
//!
//! Gate 1 orders resolution after preparation, but P08 restores the shared
//! configures MBX BEFORE any fetch: when the offline fetch probe proves all
//! locked sources present, the step skips online fetch and obligations run
//! `--offline`; a cold/incomplete home fetches through this explicit path and
//! records the miss in the job log. Steps are gated on lockfile presence
//! (lockless emits nothing).

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::Step;
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::OrchestratorError;
use crate::discover::{PlannedWorkspace, workspace_lock, workspace_manifest};

#[path = "source_transport.rs"]
mod transport;
pub(crate) use transport::SourceTransportAdmission;

#[path = "source_producer.rs"]
pub(crate) mod producer;

#[path = "source_scope.rs"]
mod scope;
pub(crate) use scope::selected_fetch_roots;

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

/// Fetch steps for crate jobs: probe, skip when warm, else fetch.
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

/// Fetch steps for the plan job.
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
/// The script runs `cargo fetch --locked --offline` first: success
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
        let script = fetch_script(catalog, root)?;
        let manifest = workspace_manifest(root);
        let name = if root.is_empty() {
            FETCH_SOURCES_STEP.to_owned()
        } else {
            format!("{FETCH_SOURCES_STEP} ({manifest})")
        };
        steps.push(
            velnor_actions_workflow_renderer::shell_step(
                &name,
                vec!["sh".to_owned(), "-c".to_owned(), script],
                env.clone(),
            )
            .map_err(|err| OrchestratorError::Contract {
                problem: err.to_string(),
            })?,
        );
    }
    Ok(steps)
}

/// Fixed probe-and-fetch script for one selected locked workspace.
///
/// Cargo 1.98.1 fetch supports target filtering, but no package or feature
/// selection. Fetching the complete locked workspace is the sound fallback:
/// unlike default-feature metadata, it checks optional and target-specific
/// sources that later offline obligations may need. The same Cargo operation
/// owns the offline probe and online fill; neither compiles repository code.
/// Inputs are selected workspace roots, never a recursive manifest glob.
///
/// Runs from the checkout, matching the obligation Cargo config search.
/// The shell-step constructor unsets credentials before repository source
/// replacement or credential-provider configuration can be consumed. Mise
/// remains isolated from repository configuration, environment, and hooks.
fn fetch_script(catalog: &ToolCatalog, root: &str) -> Result<String, OrchestratorError> {
    let spec = catalog.tool_spec(PinnedTool::Rust)?;
    let base = format!("mise --no-config --no-env --no-hooks exec {spec} -- cargo");
    let manifest = format!(" {}", isolated_manifest_flag(root));
    let probe = format!("{base} fetch --locked --offline{manifest} >/dev/null 2>&1");
    let fetch = format!("{base} fetch --locked{manifest}");
    Ok(format!(
        "{}if {probe}; then echo \"velnor: sources hit, skipping fetch\"; else echo \"velnor: sources miss (source_missing), fetching complete_locked_workspace\"; {fetch}; fi",
        "cd \"$GITHUB_WORKSPACE\" && "
    ))
}

/// Reject roots unsafe for shell interpolation or cache keys.
///
/// Thin error mapping over the contract single source for roots.
pub(crate) fn validate_root(root: &str) -> Result<(), OrchestratorError> {
    velnor_actions_contract::validate_fetch_root(root).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::StepKind;

    fn shell_parts(kind: &StepKind) -> Option<(&Vec<String>, &BTreeMap<String, String>)> {
        match kind {
            StepKind::Shell { run, env } => Some((run, env)),
            _ => None,
        }
    }

    #[test]
    fn root_step_probes_before_fetch_with_miss_record() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps_for_plan(&catalog, &[String::new()]).expect("fetch steps");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].name, FETCH_SOURCES_STEP);
        let (run, env) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
        assert_eq!(&run[..2], ["sh", "-c"]);
        let spec = catalog
            .tool_spec(PinnedTool::Rust)
            .expect("qualified Rust selector");
        for need in [
            "cd \"$GITHUB_WORKSPACE\"".to_owned(),
            format!("mise --no-config --no-env --no-hooks exec {spec} -- cargo"),
            "fetch --locked --offline".to_owned(),
            "cargo fetch --locked".to_owned(),
            "--manifest-path \"$GITHUB_WORKSPACE/Cargo.toml\"".to_owned(),
            "sources hit, skipping fetch".to_owned(),
            "sources miss (source_missing)".to_owned(),
        ] {
            assert!(run[2].contains(&need), "script misses {need}: {}", run[2]);
        }
        assert!(
            env.get("MISE_CARGO_HOME").is_some_and(|v| !v.is_empty()),
            "plan fetch uses owned homes"
        );
    }

    #[test]
    fn crate_fetch_carries_full_validated_contract() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps_for_crate(&catalog, &[String::new()]).expect("fetch steps");
        let (_, got) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
        for (key, value) in [
            ("MISE_NO_CONFIG", "1"),
            ("MISE_NO_ENV", "1"),
            ("MISE_NO_HOOKS", "1"),
            ("MISE_LOCKFILE", "0"),
            ("MISE_AUTO_INSTALL", "false"),
            ("MISE_EXEC_AUTO_INSTALL", "false"),
        ] {
            assert_eq!(
                got.get(key).map(String::as_str),
                Some(value),
                "crate fetch must carry the validated policy pair {key}"
            );
        }
        for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
            assert!(
                got.get(key).is_some_and(|value| !value.is_empty()),
                "crate fetch must carry a non-empty {key}"
            );
        }
        for key in [
            "MISE_GITHUB_TOKEN",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "ACTIONS_RUNTIME_TOKEN",
        ] {
            assert!(
                got.get(key).is_some_and(String::is_empty),
                "crate fetch must scrub credential {key}"
            );
        }
        let shared = crate::matrix_step::task_step_env(&catalog, &BTreeMap::new(), true)
            .expect("shared crate env");
        for (key, value) in &shared {
            assert_eq!(got.get(key), Some(value), "fetch contract {key}");
        }
    }

    #[test]
    fn plan_fetch_uses_owned_homes() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps_for_plan(&catalog, &[String::new()]).expect("fetch steps");
        let (_, got) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
        let shared = crate::matrix_step::task_step_env(&catalog, &BTreeMap::new(), true)
            .expect("shared env");
        for (key, value) in &shared {
            assert_eq!(got.get(key), Some(value), "plan fetch contract {key}");
        }
    }

    #[test]
    fn nested_step_names_its_manifest() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps_for_plan(&catalog, &["nested".to_owned()]).expect("fetch steps");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].name, "Fetch Cargo sources (nested/Cargo.toml)");
        let (run, _) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
        assert!(
            run[2].contains("--manifest-path \"$GITHUB_WORKSPACE/nested/Cargo.toml\""),
            "script names absolute manifest: {}",
            run[2]
        );
    }

    #[test]
    fn lockless_roots_emit_no_steps() {
        let catalog = ToolCatalog::pinned();
        let crates = fetch_steps_for_crate(&catalog, &[]).expect("crate fetch steps");
        let plan = fetch_steps_for_plan(&catalog, &[]).expect("plan fetch steps");
        assert!(crates.is_empty() && plan.is_empty());
    }
}

#[cfg(test)]
#[path = "source_prep_behavior_tests.rs"]
mod behavior_tests;
