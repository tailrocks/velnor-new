//! Install lockfile audit glue (G2): lock-file hygiene for local installs.
//!
//! The audit verifies that each local `mise install` spec resolves to
//! a well-formed lock entry; CI-platform holes and corrupt entries
//! block `generate`, while `plan` reports findings without failing.
//!
//! Both install-class emissions are audited: `Prepare pinned tools`
//! steps from the IR (catalog specs) and validator install commands
//! from the typed render context (validator pins). Validator steps
//! materialize at render time, so scraping the IR alone would leave
//! validator ambient installs invisible; the typed commands close that
//! gap without string-matching rendered YAML.
//!
//! Hygiene only, not runtime verification: isolated CI installs trust
//! upstream TLS and exact pins. Checksums are TOFU; the audit verifies
//! lock coverage and shape but cannot recompute bytes or detect a
//! malicious self-consistent lock.

use std::io::Read;
use std::path::Path;

use velnor_actions_contract::{ReleaseTarget, Step, StepKind, StepRole, WorkflowIr};
use velnor_actions_mise::toolfiles::lockfile::{
    InstallCoverage, InstallSubject, audit_install_coverage, mise_platform_for_target,
    parse_mise_lockfile, subject_for_install_spec,
};
use velnor_actions_mise::{MISE_LOCK_FILE, ToolCatalog};
use velnor_actions_workflow_renderer::render::ValidatorCommand;

use crate::verify::validator_install_pin;
use crate::workflow::verifier_actions::VerifierActionSet;
#[path = "lock_audit_names.rs"]
mod names;
use names::subject_names;
#[path = "lock_audit_rust.rs"]
mod rust;
use rust::{is_catalogued_jactionlint, is_rust, rust_manifest_coverage};
#[path = "lock_audit_vectors.rs"]
mod vectors;
use vectors::classify_mise_vectors;

/// Audit outcome: one advisory summary plus fail-closed diagnostics.
pub(crate) struct LockAuditOutcome {
    /// Advisory coverage summary, when anything is unverified.
    pub recommendation: Option<String>,
    /// Fail-closed diagnostics (holes, corrupt shapes, unauditable vectors).
    pub blocking: Vec<String>,
}

/// Largest tool file the audit reads: real locks are KBs, so anything
/// past this is hostile or corrupt and blocks instead of loading
/// unbounded into `prepare`/`generate`.
const MAX_TOOL_FILE_BYTES: u64 = 1024 * 1024;

/// What one validator command contributes to the install audit.
enum CommandInstalls {
    /// No `mise` vector needing audit: no `mise` token, or only
    /// isolated `exec` vectors. Not audited.
    NotInstall,
    /// Emitted install specs, possibly empty (the caller blocks bare).
    Specs(Vec<String>),
    /// A `mise` token with an unclassifiable tail (the caller blocks
    /// with the detail; silence would be a fail-open).
    Unclassifiable(String),
}

/// Classification of the `mise` vectors in one argv token run.
enum MiseVectors {
    /// No `mise` token, or only isolated `exec` vectors.
    NotInstall,
    /// The first `install` vector's specs, possibly empty.
    Install(Vec<String>),
    /// A `mise` token with an unclassifiable tail (unknown subcommand
    /// or truncated argv); detail feeds the blocking diagnostic.
    Unclassifiable(String),
}

/// Audit emitted install sets against `root/mise.lock` using pinned defaults.
#[cfg(test)]
pub(crate) fn audit_prepare_installs(
    root: &Path,
    ir: &WorkflowIr,
    label: &str,
    validator_commands: &[ValidatorCommand],
) -> LockAuditOutcome {
    audit_prepare_installs_with_catalog(root, ir, label, validator_commands, &ToolCatalog::pinned())
}

/// Audit emitted install sets against `root/mise.lock` with a resolved [`ToolCatalog`].
#[cfg(test)]
pub(crate) fn audit_prepare_installs_with_catalog(
    root: &Path,
    ir: &WorkflowIr,
    label: &str,
    validator_commands: &[ValidatorCommand],
    catalog: &ToolCatalog,
) -> LockAuditOutcome {
    let verifiers =
        match VerifierActionSet::build(&catalog.for_emission(), env!("CARGO_PKG_VERSION")) {
            Ok(verifiers) => verifiers,
            Err(error) => {
                return LockAuditOutcome {
                    recommendation: None,
                    blocking: vec![format!("verifier_action_templates_invalid:{error}")],
                };
            }
        };
    audit_prepare_installs_with_catalog_and_verifiers(
        root,
        ir,
        label,
        validator_commands,
        catalog,
        &verifiers,
    )
}

/// Audit installs against the resolved catalog and the exact verifier action
/// templates that generation will emit.
pub(crate) fn audit_prepare_installs_with_catalog_and_verifiers(
    root: &Path,
    ir: &WorkflowIr,
    label: &str,
    validator_commands: &[ValidatorCommand],
    catalog: &ToolCatalog,
    verifiers: &VerifierActionSet,
) -> LockAuditOutcome {
    let mut blocking = Vec::new();
    let mut subjects: Vec<InstallSubject> = Vec::new();
    for (id, job) in &ir.jobs {
        let positions = verifier_positions(id, job, verifiers, &mut blocking);
        let mut rust_sequence_reported = false;
        let mut jactionlint_sequence_reported = false;
        for (step_index, step) in job.steps.iter().enumerate() {
            if step.role != Some(StepRole::PreparePinnedTools) {
                continue;
            }
            let Some(specs) = prepare_specs(id, step, &mut blocking) else {
                continue;
            };
            if specs.is_empty() {
                blocking.push(bare_install(id));
                continue;
            }
            for spec in specs {
                // Emitted specs carry project references (hard decision 5);
                // audit the concrete versions the project declares.
                let spec = catalog.materialize(&spec);
                match subject_for_install_spec(&spec, catalog) {
                    Some(subject) if is_rust(&subject) => {
                        if rust_verifier_sequence_valid(job, &positions, Some(step_index)) {
                            rust_manifest_coverage(&subject, &mut blocking);
                        } else {
                            if !rust_sequence_reported {
                                blocking.push(format!(
                                    "rust_verifier_sequence_invalid:{id}:expected restored immediately before prepare and installed after toolchain setup"
                                ));
                                rust_sequence_reported = true;
                            }
                            subjects.push(subject);
                        }
                    }
                    // `Verify jactionlint` checks the installed executable
                    // against the catalog digest table, so a catalogued
                    // version needs no `mise.lock` row to be verified.
                    Some(subject) if is_catalogued_jactionlint(&subject) => {
                        if jactionlint_verifier_sequence_valid(&positions, Some(step_index)) {
                            // The exact digest verifier follows the install.
                        } else {
                            if !jactionlint_sequence_reported {
                                blocking.push(format!(
                                    "jactionlint_verifier_sequence_invalid:{id}:expected immediately after prepare pinned tools"
                                ));
                                jactionlint_sequence_reported = true;
                            }
                            subjects.push(subject);
                        }
                    }
                    Some(subject) => subjects.push(subject),
                    None => blocking.push(format!("unauditable_install_spec:{spec}")),
                }
            }
        }
    }
    for command in validator_commands {
        audit_validator_command(command, &mut subjects, &mut blocking);
    }
    subjects.sort_by(|left, right| left.display.cmp(&right.display));
    subjects.dedup();
    if subjects.is_empty() && blocking.is_empty() {
        return LockAuditOutcome {
            recommendation: None,
            blocking,
        };
    }
    let Some(platform) = ReleaseTarget::for_runner_label(label)
        .map(ReleaseTarget::triple)
        .and_then(mise_platform_for_target)
        .map(str::to_owned)
    else {
        return LockAuditOutcome {
            recommendation: Some(format!(
                "tool_install_unverified:cannot audit installs (unsupported runner label {label})"
            )),
            blocking,
        };
    };
    let lock_text = capped_read(&root.join(MISE_LOCK_FILE), MISE_LOCK_FILE, &mut blocking);
    audit_against_lock(lock_text.as_deref(), &subjects, &platform, blocking)
}

#[derive(Default)]
struct VerifierPositions {
    restored_rust: Vec<usize>,
    installed_rust: Vec<usize>,
    jactionlint: Vec<usize>,
}

fn verifier_positions(
    id: &str,
    job: &velnor_actions_contract::Job,
    verifiers: &VerifierActionSet,
    blocking: &mut Vec<String>,
) -> VerifierPositions {
    let mut positions = VerifierPositions::default();
    for (index, step) in job.steps.iter().enumerate() {
        let action = match &step.kind {
            StepKind::Action { uses, .. } => verifiers.by_uses(uses),
            StepKind::Shell { .. } | StepKind::TaskExecution { .. } | StepKind::Internal { .. } => {
                None
            }
        };
        if let Some(action) = action {
            if !VerifierActionSet::canonical_call(step, action) {
                blocking.push(format!("noncanonical_verifier_call:{id}:{}", action.uses));
                continue;
            }
            match action.uses {
                uses if uses == verifiers.restored_rust.uses => {
                    positions.restored_rust.push(index);
                }
                uses if uses == verifiers.installed_rust.uses => {
                    positions.installed_rust.push(index);
                }
                uses if uses == verifiers.jactionlint.uses => {
                    positions.jactionlint.push(index);
                }
                _ => unreachable!("lookup returns only registered verifier actions"),
            }
        } else if step.role == Some(StepRole::VerifyRustManifest)
            || matches!(
                step.name.as_str(),
                crate::workflow::VERIFY_RESTORED_RUST_STEP
                    | crate::workflow::VERIFY_RUST_MANIFEST_STEP
                    | crate::workflow::VERIFY_JACTIONLINT_STEP
            )
        {
            blocking.push(format!("forged_verifier_call:{id}:{}", step.name));
        }
    }
    if positions.restored_rust.len() > 1 {
        blocking.push(format!("duplicate_verifier_call:{id}:restored-rust"));
    }
    if positions.installed_rust.len() > 1 {
        blocking.push(format!("duplicate_verifier_call:{id}:installed-rust"));
    }
    if positions.jactionlint.len() > 1 {
        blocking.push(format!("duplicate_verifier_call:{id}:jactionlint"));
    }
    positions
}

fn rust_verifier_sequence_valid(
    job: &velnor_actions_contract::Job,
    positions: &VerifierPositions,
    prepare_at: Option<usize>,
) -> bool {
    let (Some(prepare_at), [restored_at], [installed_at]) = (
        prepare_at,
        positions.restored_rust.as_slice(),
        positions.installed_rust.as_slice(),
    ) else {
        return false;
    };
    let components_at = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::PrepareRustComponents));
    restored_at.checked_add(1) == Some(prepare_at)
        && *installed_at > prepare_at
        && components_at.is_none_or(|at| *installed_at > at)
}

fn jactionlint_verifier_sequence_valid(
    positions: &VerifierPositions,
    prepare_at: Option<usize>,
) -> bool {
    matches!(
        (prepare_at, positions.jactionlint.as_slice()),
        (Some(prepare_at), [verify_at]) if verify_at == &(prepare_at + 1)
    )
}

/// Bare-install diagnostic: zero specs means versions would come from
/// `mise.toml`, so silence would be a fail-open.
fn bare_install(id: &str) -> String {
    format!(
        "bare_install_step:{id} installs zero specs (versions would come from mise.toml); emit explicit specs"
    )
}

/// Specs from a Prepare step's plain argv; a Prepare step always
/// installs, so a missing or unclassifiable vector blocks as a
/// generator bug.
fn prepare_specs(id: &str, step: &Step, blocking: &mut Vec<String>) -> Option<Vec<String>> {
    let StepKind::Shell { run, .. } = &step.kind else {
        blocking.push(format!("unauditable_prepare_argv:{id}:not_a_shell_step"));
        return None;
    };
    let tokens: Vec<&str> = run.iter().map(String::as_str).collect();
    match classify_mise_vectors(&tokens) {
        MiseVectors::Install(specs) => Some(specs),
        MiseVectors::NotInstall => {
            blocking.push(format!("unauditable_prepare_argv:{id}:no_install"));
            None
        }
        MiseVectors::Unclassifiable(detail) => {
            blocking.push(format!("unauditable_prepare_argv:{id}:{detail}"));
            None
        }
    }
}

/// Audit one validator command's install contribution.
///
/// Isolated `exec` vectors contribute nothing; install specs resolve
/// through the validator pin only (a catalog spec here blocks as
/// misplaced instead of auditing through the wrong pin set), and an
/// unclassifiable `mise` vector blocks instead of passing silently.
fn audit_validator_command(
    command: &ValidatorCommand,
    subjects: &mut Vec<InstallSubject>,
    blocking: &mut Vec<String>,
) {
    for argv in [&command.prepare_argv, &command.argv] {
        match validator_argv_specs(argv) {
            CommandInstalls::NotInstall => {}
            CommandInstalls::Unclassifiable(detail) => {
                blocking.push(format!(
                    "unauditable_validator_argv:{}:{detail}",
                    command.name
                ));
            }
            CommandInstalls::Specs(specs) => {
                if specs.is_empty() {
                    blocking.push(bare_install(&command.name));
                    continue;
                }
                for spec in specs {
                    match validator_subject(&spec) {
                        Some(subject) => subjects.push(subject),
                        None => blocking.push(format!("unauditable_install_spec:{spec}")),
                    }
                }
            }
        }
    }
}

/// Install contribution of one validator command.
///
/// Scans the typed argv (plain vectors and `sh -c` scripts alike) and
/// classifies each `mise` vector: `install` extracts, isolated `exec`
/// contributes nothing, anything else blocks as unclassifiable.
fn validator_argv_specs(argv: &[String]) -> CommandInstalls {
    let mut tokens = Vec::new();
    for arg in argv {
        if looks_like_script(arg) {
            tokens.extend(arg.split_whitespace());
        } else {
            tokens.push(arg.as_str());
        }
    }
    match classify_mise_vectors(&tokens) {
        MiseVectors::Install(specs) => CommandInstalls::Specs(specs),
        MiseVectors::NotInstall => CommandInstalls::NotInstall,
        MiseVectors::Unclassifiable(detail) => CommandInstalls::Unclassifiable(detail),
    }
}

/// True for argv elements holding a shell script rather than one token.
fn looks_like_script(arg: &str) -> bool {
    arg.chars().any(char::is_whitespace)
}

/// Resolve one emitted validator spec through the validator pin.
fn validator_subject(spec: &str) -> Option<InstallSubject> {
    let (name, version, lock_key) = validator_install_pin(spec)?;
    Some(InstallSubject {
        display: format!("{name}@{version}"),
        expected_version: version.to_owned(),
        lock_key: lock_key.to_owned(),
    })
}

/// Read a tool file capped at [`MAX_TOOL_FILE_BYTES`].
///
/// Missing files are `None` (the caller decides); oversized or
/// unreadable files block instead of loading unbounded or passing
/// silently behind a missing-file advisory. The cap binds the read
/// itself (`take(MAX + 1)` first), never a pre-read metadata size, so
/// a concurrent grow cannot smuggle unbounded bytes past the check.
fn capped_read(path: &Path, file: &str, blocking: &mut Vec<String>) -> Option<String> {
    let handle = match std::fs::File::open(path) {
        Ok(handle) => handle,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            blocking.push(format!("unreadable_tool_file:{file}:{err}"));
            return None;
        }
    };
    let mut text = String::new();
    let mut capped = handle.take(MAX_TOOL_FILE_BYTES + 1);
    if let Err(err) = capped.read_to_string(&mut text) {
        blocking.push(format!("unreadable_tool_file:{file}:{err}"));
        return None;
    }
    if u64::try_from(text.len()).is_ok_and(|len| len > MAX_TOOL_FILE_BYTES) {
        blocking.push(format!(
            "oversized_tool_file:{file}:{} exceeds {MAX_TOOL_FILE_BYTES} bytes",
            text.len()
        ));
        return None;
    }
    Some(text)
}

/// Audit resolved subjects against lock bytes (missing and malformed loud).
fn audit_against_lock(
    lock_text: Option<&str>,
    subjects: &[InstallSubject],
    platform: &str,
    mut blocking: Vec<String>,
) -> LockAuditOutcome {
    let names = subject_names(subjects);
    let Some(text) = lock_text else {
        return LockAuditOutcome {
            recommendation: Some(format!(
                "tool_install_unverified:{} installs skip verification (no mise.lock): {names}; install on the CI platform and commit the resulting lock",
                subjects.len(),
            )),
            blocking,
        };
    };
    let lock = match parse_mise_lockfile(text) {
        Ok(lock) => lock,
        Err(problem) => {
            return LockAuditOutcome {
                recommendation: Some(format!(
                    "tool_install_unverified:{} installs skip verification (mise.lock malformed: {problem}); fix the lockfile to verify them",
                    subjects.len(),
                )),
                blocking,
            };
        }
    };
    let mut gaps = Vec::new();
    for (subject, coverage) in subjects
        .iter()
        .zip(audit_install_coverage(&lock, subjects, platform))
    {
        match coverage {
            InstallCoverage::Verified => {}
            InstallCoverage::VersionDrift { locked } => {
                gaps.push(format!(
                    "{}(version_drift:locked {locked})",
                    subject.display
                ));
            }
            InstallCoverage::MissingEntry => {
                gaps.push(format!("{}(no_entry)", subject.display));
            }
            InstallCoverage::NoChecksums => {
                gaps.push(format!("{}(no_checksums)", subject.display));
            }
            InstallCoverage::MissingPlatform { locked_platforms } => {
                blocking.push(format!(
                    "lock_missing_platform:{} locks [{}] without {platform}; run 'mise lock' and commit the resulting checksums; if a backend publishes no checksums, install on {platform} and commit those",
                    subject.display,
                    locked_platforms.join(", "),
                ));
            }
            InstallCoverage::CorruptChecksum { observed } => {
                blocking.push(format!(
                    "lock_corrupt_checksum:{} {platform} checksum malformed ({observed}); reinstall on {platform} and commit the lock",
                    subject.display
                ));
            }
        }
    }
    let recommendation = (!gaps.is_empty()).then(|| {
        format!(
            "tool_install_unverified:{} installs skip verification: {}; install on the CI platform and commit the lock to verify them",
            gaps.len(),
            gaps.join(", "),
        )
    });
    LockAuditOutcome {
        recommendation,
        blocking,
    }
}

#[cfg(test)]
#[path = "lock_audit_tests.rs"]
mod lock_audit_tests;
