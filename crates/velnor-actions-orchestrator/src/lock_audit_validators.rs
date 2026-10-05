//! Audit validator install vectors against the selected validator pin.

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_mise::toolfiles::lockfile::InstallSubject;
use velnor_actions_workflow_renderer::render::ValidatorCommand;

use crate::vectors::validator_install_pin;

/// What one validator command contributes to the install audit.
enum CommandInstalls {
    /// No `mise` vector needing audit: no `mise` token, or only isolated `exec` vectors.
    NotInstall,
    /// Emitted install specs, possibly empty (the caller blocks bare).
    Specs(Vec<String>),
    /// A `mise` token with an unclassifiable tail.
    Unclassifiable(String),
}

/// Classification of the `mise` vectors in one argv token run.
enum MiseVectors {
    /// No `mise` token, or only isolated `exec` vectors.
    NotInstall,
    /// The first `install` vector's specs, possibly empty.
    Install(Vec<String>),
    /// An unrecognized or truncated `mise` vector.
    Unclassifiable(String),
}

/// Bare-install diagnostic: zero specs means versions would come from `mise.toml`.
pub(super) fn bare_install(id: &str) -> String {
    format!(
        "bare_install_step:{id} installs zero specs (versions would come from mise.toml); emit explicit specs"
    )
}

/// Specs from a Prepare step's plain argv; missing installs fail closed.
pub(super) fn prepare_specs(
    id: &str,
    step: &Step,
    blocking: &mut Vec<String>,
) -> Option<Vec<String>> {
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

/// Audit both a validator's cold preparation and its command vector.
pub(super) fn audit_validator_command(
    command: &ValidatorCommand,
    subjects: &mut Vec<InstallSubject>,
    blocking: &mut Vec<String>,
) {
    if !command.prepare_argv.is_empty() {
        audit_validator_argv(
            &command.name,
            &command.prepare_argv,
            "prepare",
            subjects,
            blocking,
        );
    }
    audit_validator_argv(&command.name, &command.argv, "run", subjects, blocking);
}

/// Audit install vectors from one emitted validator argv.
fn audit_validator_argv(
    name: &str,
    argv: &[String],
    phase: &str,
    subjects: &mut Vec<InstallSubject>,
    blocking: &mut Vec<String>,
) {
    match validator_command_specs(argv) {
        CommandInstalls::NotInstall => {}
        CommandInstalls::Unclassifiable(detail) => {
            let scope = if phase == "run" {
                "validator_argv"
            } else {
                "validator_prepare_argv"
            };
            blocking.push(format!("unauditable_{scope}:{name}:{detail}"));
        }
        CommandInstalls::Specs(specs) => {
            if specs.is_empty() {
                blocking.push(bare_install(name));
                return;
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

/// Install contribution of one validator argv, including shell-script elements.
fn validator_command_specs(argv: &[String]) -> CommandInstalls {
    let mut tokens = Vec::new();
    for arg in argv {
        if arg.chars().any(char::is_whitespace) {
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

/// Classify Mise vectors; unrecognized or truncated forms are not silently ignored.
fn classify_mise_vectors(tokens: &[&str]) -> MiseVectors {
    for (at, token) in tokens.iter().enumerate() {
        if *token != "mise" {
            continue;
        }
        let mut cursor = at + 1;
        while cursor < tokens.len() && tokens[cursor].starts_with('-') {
            cursor += 1;
        }
        let Some(subcommand) = tokens.get(cursor) else {
            return MiseVectors::Unclassifiable("truncated".to_owned());
        };
        match *subcommand {
            "install" => return MiseVectors::Install(specs_until_metachar(&tokens[cursor + 1..])),
            "exec" => {}
            other => {
                return MiseVectors::Unclassifiable(format!("unexpected_subcommand:{other}"));
            }
        }
    }
    MiseVectors::NotInstall
}

/// Specs until the first shell metacharacter (scripts) or argv end.
fn specs_until_metachar(tail: &[&str]) -> Vec<String> {
    let mut specs = Vec::new();
    for spec in tail {
        if matches!(*spec, "&&" | ";" | "||" | "}" | "{" | "(" | ")") {
            break;
        }
        specs.push((*spec).to_owned());
    }
    specs
}

/// Resolve one emitted validator spec through the validator pin.
pub(crate) fn validator_subject(spec: &str) -> Option<InstallSubject> {
    let (name, version, lock_key) = validator_install_pin(spec)?;
    Some(InstallSubject {
        display: format!("{name}@{version}"),
        expected_version: version.to_owned(),
        lock_key: lock_key.to_owned(),
    })
}
