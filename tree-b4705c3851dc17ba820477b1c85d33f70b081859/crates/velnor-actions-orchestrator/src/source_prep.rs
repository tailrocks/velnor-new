//! Plan- and task-job Cargo source preparation ahead of locked/offline consumers.
//!
//! Gate 1 orders resolution after preparation: `cargo fetch --locked`
//! populates every target's sources from the network once, so the later
//! locked/offline qualification inside `generate` (and `plan`) succeeds
//! from a cold runner. Steps are gated on lockfile presence: lockless
//! workspaces have nothing pinned, so `fetch --locked` (which refuses to
//! create a lock) is omitted exactly where qualification is skipped.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

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

/// One `cargo fetch --locked` step per lockful workspace root for task jobs.
///
/// Fetched sources land in the `MISE_CARGO_HOME` this step runs with,
/// so the env is built by the same validated constructor `Run task`
/// uses: fetch and consumer match by construction, never by a caller
/// passing the right map.
/// # Errors
///
/// Returns contract/render errors when the Mise adapter or the step-env
/// contract rejects the request.
pub(crate) fn fetch_steps_for_task(
    catalog: &ToolCatalog,
    roots: &[String],
) -> Result<Vec<Step>, OrchestratorError> {
    let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new())?;
    fetch_steps_with(catalog, roots, &env)
}

/// One `cargo fetch --locked` step per lockful workspace root for plan jobs.
///
/// The plan consumer runs with ambient homes, so plan fetch keeps the
/// empty map its helper inherits: no owned homes, no divergence.
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
pub(crate) fn fetch_steps_for_plan(
    catalog: &ToolCatalog,
    roots: &[String],
) -> Result<Vec<Step>, OrchestratorError> {
    fetch_steps_with(catalog, roots, &BTreeMap::new())
}

/// One `cargo fetch --locked` step per lockful workspace root.
///
/// The root workspace keeps the minimal argv; nested workspaces name
/// their manifest explicitly and carry it in the step name.
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
fn fetch_steps_with(
    catalog: &ToolCatalog,
    roots: &[String],
    env: &BTreeMap<String, String>,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::with_capacity(roots.len());
    for root in roots {
        let manifest = workspace_manifest(root);
        let mut args = vec![OsString::from("fetch"), OsString::from("--locked")];
        if !root.is_empty() {
            args.push(OsString::from("--manifest-path"));
            args.push(OsString::from(&manifest));
        }
        let program = OsString::from("cargo");
        let exec = PinnedToolExec::new(vec![PinnedTool::Rust], &program, args).map_err(|err| {
            OrchestratorError::Contract {
                problem: err.to_string(),
            }
        })?;
        let run = strings_of(exec.argv(catalog))
            .map_err(|problem| OrchestratorError::Contract { problem })?;
        let name = if root.is_empty() {
            FETCH_SOURCES_STEP.to_owned()
        } else {
            format!("{FETCH_SOURCES_STEP} ({manifest})")
        };
        steps.push(Step {
            name,
            kind: StepKind::Shell {
                run,
                env: env.clone(),
            },
        });
    }
    Ok(steps)
}

/// Convert fixed argv to UTF-8 strings.
fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(argv.len());
    for arg in argv {
        match arg.into_string() {
            Ok(text) => out.push(text),
            Err(_) => return Err("non_utf8_argv".to_owned()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell_parts(kind: &StepKind) -> Option<(&Vec<String>, &BTreeMap<String, String>)> {
        match kind {
            StepKind::Shell { run, env } => Some((run, env)),
            _ => None,
        }
    }

    #[test]
    fn root_step_vector_is_byte_exact() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps_for_plan(&catalog, &[String::new()]).expect("fetch steps");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].name, FETCH_SOURCES_STEP);
        let want: Vec<String> = [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            &catalog.tool_spec(PinnedTool::Rust),
            "--",
            "cargo",
            "fetch",
            "--locked",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        let (run, env) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
        assert_eq!(run, &want);
        assert!(env.is_empty(), "plan fetch keeps ambient homes");
    }

    #[test]
    fn task_fetch_carries_full_validated_contract() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps_for_task(&catalog, &[String::new()]).expect("fetch steps");
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
                "task fetch must carry the validated policy pair {key}"
            );
        }
        for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
            assert!(
                got.get(key).is_some_and(|value| !value.is_empty()),
                "task fetch must carry a non-empty {key}"
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
                "task fetch must never carry a credential {key}"
            );
        }
        let shared =
            crate::matrix_step::task_step_env(&catalog, &BTreeMap::new()).expect("shared task env");
        assert_eq!(got, &shared, "fetch must match Run task by construction");
    }

    #[test]
    fn plan_fetch_keeps_empty_ambient_contract() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps_for_plan(&catalog, &[String::new()]).expect("fetch steps");
        let (_, got) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
        assert!(got.is_empty(), "plan fetch keeps ambient homes");
    }

    #[test]
    fn nested_step_names_its_manifest() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps_for_plan(&catalog, &["nested".to_owned()]).expect("fetch steps");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].name, "Fetch Cargo sources (nested/Cargo.toml)");
        let (run, _) = shell_parts(&steps[0].kind).expect("fetch must be a shell step");
        assert_eq!(
            &run[run.len() - 2..],
            ["--manifest-path", "nested/Cargo.toml"]
        );
    }

    #[test]
    fn lockless_roots_emit_no_steps() {
        let catalog = ToolCatalog::pinned();
        let task = fetch_steps_for_task(&catalog, &[]).expect("task fetch steps");
        let plan = fetch_steps_for_plan(&catalog, &[]).expect("plan fetch steps");
        assert!(task.is_empty() && plan.is_empty());
    }
}
