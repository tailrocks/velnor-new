//! Rust input closures: complete first-party inputs, explicit unknowns.
//!
//! Unknown inputs forbid reuse and coverage. The kind-gated classes,
//! Cargo filenames, and Rust source extensions here are Rust-domain
//! rules; the orchestrator dispatches per stack to this resolver.

use std::path::Path;

use velnor_actions_contract::{
    ClosureBuilder, ContractError, ProposedTask, Provenance, TaskInputClosure,
};

use super::closure_probes::{
    probe_cargo_config, probe_declared, probe_file, probe_lockfile, probe_nextest_config,
};
use crate::task_identity::DigestSlot;
use crate::tasks::TaskKind;

/// Resolve one proposed task's closure against the checkout at `root`.
///
/// Reads adapter facts from the proposal (unit path, features, target,
/// configuration, drivers, kind, declared inputs); filesystem probes
/// bind content, proven absence, or explicit unknowns.
///
/// # Errors
///
/// Returns [`ContractError`] for task-kind spellings outside the seven
/// known kinds.
pub fn resolve_closure_at_root(
    root: &Path,
    task: &ProposedTask,
    profile_nextest_config: Option<&str>,
    graph_digest: &str,
    toolchain_id: &str,
    platform_id: &str,
    checkout_inputs: &crate::semantic_inputs::SemanticInventory,
) -> Result<TaskInputClosure, ContractError> {
    TaskKind::parse(&task.task_kind)?;
    let manifest = task.identity.unit_path.as_str();
    let nextest = probe_nextest_config(root, profile_nextest_config);
    let mut closure = ClosureBuilder::new()
        .input(
            "source_tree",
            crate::semantic_inputs::resolve(root, task, checkout_inputs),
        )
        .input("manifest", probe_file(root, manifest))
        .input("lockfile", probe_lockfile(root, manifest))
        .input("nextest_config", nextest)
        .input("cargo_config", probe_cargo_config(root, manifest))
        .digest("local_deps", graph_digest)
        .digest("toolchain", toolchain_id)
        .digest("platform", platform_id)
        .value("features", &task.identity.features.join(","))
        .value("target", &task.identity.target)
        .value("profile", &task.configuration)
        .value("driver", &task.identity.compile_driver)
        .value("runner", &task.identity.test_runner)
        .value("kind", &task.task_kind);
    for (index, extra) in task.identity.declared_inputs.iter().enumerate() {
        let name = format!("declared_extra:{index}:{extra}");
        closure = closure.input(&name, probe_declared(root, extra));
    }
    let closure = closure.input("vcs", vcs_provenance(task.identity.undeclared_reads));
    Ok(closure.build(&task.task_id))
}

/// Digest slot of a [`Provenance`], preserving absence distinctly.
///
/// Externally guarded inputs have no producer today; if one ever
/// appears it maps to unknown (fail-closed: unverified by us).
fn digest_slot(provenance: Provenance) -> DigestSlot {
    match provenance {
        Provenance::Known { digest } => DigestSlot::Known(digest),
        Provenance::AbsentProven { evidence } => DigestSlot::AbsentProven(evidence),
        Provenance::Unknown { reason } => DigestSlot::Unknown(reason),
        Provenance::GuardedExternally { guard } => {
            DigestSlot::Unknown(format!("guarded_externally:{guard}"))
        }
    }
}

/// Lockfile slot at `root`: content, proven absence, or unknown.
#[must_use]
pub fn lock_digest_at_root(root: &Path, manifest: &str) -> DigestSlot {
    digest_slot(probe_lockfile(root, manifest))
}

/// Nextest-config slot at `root`: content, proven absence, or unknown.
#[must_use]
pub fn nextest_digest_at_root(root: &Path, profile_config: Option<&str>) -> DigestSlot {
    digest_slot(probe_nextest_config(root, profile_config))
}

/// VCS provenance: unknown with undeclared reads, proven absent without.
fn vcs_provenance(undeclared_reads: bool) -> Provenance {
    if undeclared_reads {
        Provenance::Unknown {
            reason: "build_script_may_observe_vcs".to_owned(),
        }
    } else {
        Provenance::AbsentProven {
            evidence: "offline_cargo_no_vcs_reads".to_owned(),
        }
    }
}
