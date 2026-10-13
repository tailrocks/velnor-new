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

use velnor_actions_contract::Step;
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::OrchestratorError;
use crate::discover::{PlannedWorkspace, workspace_lock, workspace_manifest};

/// Display name of the root-workspace fetch step.
pub(crate) const FETCH_SOURCES_STEP: &str = "Fetch Cargo sources";

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
/// by construction.
/// # Errors
///
/// Returns contract errors for rejected roots or step-env failures.
pub(crate) fn fetch_steps_for_crate(
    catalog: &ToolCatalog,
    roots: &[String],
) -> Result<Vec<Step>, OrchestratorError> {
    let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new())?;
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
    let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new())?;
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
        steps.push(
            velnor_actions_workflow_renderer::ambient_shell_step(
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

/// Fixed probe-and-fetch script for one root (manifest quoted, no injection).
fn fetch_script(catalog: &ToolCatalog, root: &str) -> String {
    let spec = catalog.tool_spec(PinnedTool::Rust);
    let base = format!("mise --no-config --no-env --no-hooks exec {spec} -- cargo");
    let manifest = if root.is_empty() {
        String::new()
    } else {
        format!(" --manifest-path '{}'", workspace_manifest(root))
    };
    let probe = format!("{base} metadata --locked --offline{manifest} >/dev/null 2>&1");
    let fetch = format!("{base} fetch --locked{manifest}");
    format!(
        "if {probe}; then echo \"velnor: sources hit, skipping fetch\"; else echo \"velnor: sources miss (source_missing), fetching\"; {fetch}; fi"
    )
}

/// Reject roots unsafe for shell interpolation or cache keys.
pub(crate) fn validate_root(root: &str) -> Result<(), OrchestratorError> {
    let bad = root.contains("..")
        || root.contains('\'')
        || root.contains('"')
        || root.contains('$')
        || root.contains('`')
        || root.contains('\\')
        || root.contains('\n')
        || root.starts_with('/');
    if bad {
        return Err(OrchestratorError::Contract {
            problem: format!("unsafe_fetch_root:{root}"),
        });
    }
    Ok(())
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
        let spec = catalog.tool_spec(PinnedTool::Rust);
        for need in [
            format!("mise --no-config --no-env --no-hooks exec {spec} -- cargo"),
            "metadata --locked --offline".to_owned(),
            "cargo fetch --locked".to_owned(),
            "sources hit, skipping fetch".to_owned(),
            "sources miss (source_missing)".to_owned(),
        ] {
            assert!(run[2].contains(&need), "script misses {need}: {}", run[2]);
        }
        assert!(
            env.get("MISE_CARGO_HOME").is_some_and(|v| !v.is_empty()),
            "writer uses owned homes"
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
                !got.contains_key(key),
                "crate fetch must never carry a credential {key}"
            );
        }
        let shared = crate::matrix_step::task_step_env(&catalog, &BTreeMap::new())
            .expect("shared crate env");
        assert_eq!(
            got, &shared,
            "fetch must match obligation steps by construction"
        );
    }

    #[test]
    fn plan_fetch_uses_owned_homes_for_shared_snapshot() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps_for_plan(&catalog, &[String::new()]).expect("fetch steps");
        let (_, got) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
        let shared =
            crate::matrix_step::task_step_env(&catalog, &BTreeMap::new()).expect("shared env");
        assert_eq!(
            got, &shared,
            "writer and readers share one Cargo home expression"
        );
    }

    #[test]
    fn nested_step_names_its_manifest() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps_for_plan(&catalog, &["nested".to_owned()]).expect("fetch steps");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].name, "Fetch Cargo sources (nested/Cargo.toml)");
        let (run, _) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
        assert!(
            run[2].contains("--manifest-path 'nested/Cargo.toml'"),
            "script names manifest: {}",
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
