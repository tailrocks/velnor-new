//! Install lockfile audit glue (G2): lock-file hygiene for local installs.
//!
//! The audit verifies the committed `mise.lock` is complete and
//! well-formed for local `mise install`: every emitted install spec
//! resolves to a lock entry, and a CI-platform hole or a corrupt
//! checksum blocks `generate` with a precise remediation. `plan`
//! prints both channels and never fails; only `generate` gates on
//! blocking.
//!
//! Both install-class emissions are audited: `Prepare pinned tools`
//! steps from the IR (catalog specs) and validator preparation/run
//! installs from the typed render context (validator pins). Validator
//! preparation is a separate execution step, so ignoring its argv loses
//! cold-install coverage.
//!
//! Hygiene only, not runtime verification: CI installs run
//! `--no-config` isolated, so they never load repository config and
//! trust upstream TLS plus exact pinned versions. Lock checksums are
//! TOFU (trust on first use): the audit checks that the lock covers
//! the install set with well-formed entries — it cannot recompute
//! upstream bytes, and a self-consistent malicious lock (attacker
//! URL plus matching checksum) is a malicious commit, out of scope.

use std::io::Read;
use std::path::Path;

use velnor_actions_contract::{ReleaseTarget, StepRole, WorkflowIr};
use velnor_actions_mise::toolfiles::lockfile::{
    InstallCoverage, InstallSubject, audit_install_coverage, mise_platform_for_target,
    parse_mise_lockfile, subject_for_install_spec,
};
use velnor_actions_mise::{MISE_LOCK_FILE, ToolCatalog};
use velnor_actions_workflow_renderer::render::ValidatorCommand;

#[path = "lock_audit_names.rs"]
mod names;
use names::subject_names;
#[path = "lock_audit_validators.rs"]
mod validators;
#[cfg(test)]
pub(crate) use validators::validator_subject;
use validators::{audit_validator_command, bare_install, prepare_specs};

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

/// Audit emitted install sets against `root/mise.lock`.
///
/// Install sets come from the built workflow (what CI will install,
/// not intent): Prepare specs from the IR resolve through the
/// catalog, validator install specs from the typed render context
/// resolve through the validator pin, and anything else blocks as a
/// generator bug, never silently. A `mise` vector that is neither an
/// `install` nor an isolated `exec` blocks as unclassifiable.
pub(crate) fn audit_prepare_installs(
    root: &Path,
    ir: &WorkflowIr,
    label: &str,
    validator_commands: &[ValidatorCommand],
) -> LockAuditOutcome {
    let catalog = ToolCatalog::pinned();
    let mut blocking = Vec::new();
    let mut subjects: Vec<InstallSubject> = Vec::new();
    for (id, job) in &ir.jobs {
        for step in &job.steps {
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
                match subject_for_install_spec(&spec, &catalog) {
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
