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

/// One `cargo fetch --locked` step per lockful workspace root.
///
/// The root workspace keeps the minimal argv; nested workspaces name
/// their manifest explicitly and carry it in the step name. The step
/// env is intentionally empty: fetched sources must land in the same
/// default cargo home that the locked/offline consumers read
/// (`generate`'s internal metadata in plan, `matrix.run` in task jobs),
/// and a divergent `MISE_CARGO_HOME` here would hide them from those
/// readers.
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
pub(crate) fn fetch_steps(
    catalog: &ToolCatalog,
    roots: &[String],
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
                env: BTreeMap::new(),
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

    #[test]
    fn root_step_vector_is_byte_exact() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps(&catalog, &[String::new()]).expect("fetch steps");
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
        let StepKind::Shell { run, env } = &steps[0].kind else {
            panic!("fetch must be a shell step");
        };
        assert_eq!(run, &want);
        assert!(env.is_empty(), "fetch shares the default cargo home");
    }

    #[test]
    fn nested_step_names_its_manifest() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps(&catalog, &["nested".to_owned()]).expect("fetch steps");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].name, "Fetch Cargo sources (nested/Cargo.toml)");
        let StepKind::Shell { run, .. } = &steps[0].kind else {
            panic!("fetch must be a shell step");
        };
        assert_eq!(
            &run[run.len() - 2..],
            ["--manifest-path", "nested/Cargo.toml"]
        );
    }

    #[test]
    fn lockless_roots_emit_no_steps() {
        let catalog = ToolCatalog::pinned();
        let steps = fetch_steps(&catalog, &[]).expect("fetch steps");
        assert!(steps.is_empty());
    }
}
