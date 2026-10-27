// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::path::Path;

use velnor_actions_contract::{GeneratorLock, ReleaseTarget, StepRole, WorkflowIr, WorkflowPolicy};
use velnor_actions_mise::toolfiles::lockfile::{
    InstallCoverage, InstallSubject, audit_install_coverage, mise_platform_for_target,
    parse_mise_lockfile,
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

#[path = "lock_audit_prepare.rs"]
mod prepare_sequence;
use prepare_sequence::{
    audit_validator_command, bare_install, install_subject, jactionlint_verifier_sequence_valid,
    prepare_specs, published_helper_catalog, rust_verifier_sequence_valid, verifier_positions,
};

#[derive(Clone, Copy)]
pub(crate) struct InstallAuditAuthority<'a> {
    pub policy: WorkflowPolicy,
    pub published_generator_lock: Option<&'a GeneratorLock>,
}

pub(crate) struct LockAuditOutcome {
    pub recommendation: Option<String>,
    pub blocking: Vec<String>,
}

const MAX_TOOL_FILE_BYTES: u64 = 1024 * 1024;

enum CommandInstalls {
    NotInstall,
    Specs(Vec<String>),
    Unclassifiable(String),
}

enum MiseVectors {
    NotInstall,
    Install(Vec<String>),
    Unclassifiable(String),
}

pub(crate) fn audit_prepare_installs_with_catalog_and_verifiers(
    root: &Path,
    ir: &WorkflowIr,
    authority: InstallAuditAuthority<'_>,
    label: &str,
    validator_commands: &[ValidatorCommand],
    catalog: &ToolCatalog,
    verifiers: &VerifierActionSet,
) -> LockAuditOutcome {
    let mut blocking = Vec::new();
    let mut subjects: Vec<InstallSubject> = Vec::new();
    let helper_catalog = published_helper_catalog(authority, catalog, &mut blocking);
    for (id, job) in &ir.jobs {
        let positions = verifier_positions(id, job, verifiers, authority.policy, &mut blocking);
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
                let spec = catalog.materialize(&spec);
                match install_subject(id, step.role, &spec, catalog, helper_catalog.as_ref()) {
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
                    Some(subject) if is_catalogued_jactionlint(&subject) => {
                        if jactionlint_verifier_sequence_valid(&positions, Some(step_index)) {
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

fn looks_like_script(arg: &str) -> bool {
    arg.chars().any(char::is_whitespace)
}

fn validator_subject(spec: &str) -> Option<InstallSubject> {
    let (name, version, lock_key) = validator_install_pin(spec)?;
    Some(InstallSubject {
        display: format!("{name}@{version}"),
        expected_version: version.to_owned(),
        lock_key: lock_key.to_owned(),
    })
}

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
