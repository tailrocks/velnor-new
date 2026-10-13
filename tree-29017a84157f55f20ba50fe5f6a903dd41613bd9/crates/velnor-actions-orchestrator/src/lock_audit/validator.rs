//! Lock-audit parsing for validator-owned Mise commands.

use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::toolfiles::lockfile::{InstallSubject, subject_for_install_spec};
use velnor_actions_workflow_renderer::render::ValidatorCommand;

use crate::vectors::validator_install_pin;

use super::bare_install;

/// What one validator command contributes to the install audit.
pub(super) enum CommandInstalls {
    /// No `mise` install vector: no `mise` token, or isolated `exec` vectors.
    NotInstall,
    /// Emitted install specs, possibly empty (the caller blocks bare).
    Specs(Vec<String>),
    /// A `mise` token with an unclassifiable tail; passing silently would fail open.
    Unclassifiable(String),
}

/// Classification of the `mise` vectors in one argv token run.
pub(super) enum MiseVectors {
    /// No `mise` token, or only isolated `exec` vectors.
    NotInstall,
    /// The single `install` vector's specs, possibly empty.
    Install(Vec<String>),
    /// A `mise` token with an unclassifiable tail.
    Unclassifiable(String),
}

/// Audit one validator command's install contribution.
///
/// Isolated `exec` vectors contribute nothing. Catalog tools resolve
/// through the catalog, validator-only tools through exact pins; malformed
/// vectors block instead of passing silently.
pub(super) fn audit_validator_command(
    command: &ValidatorCommand,
    catalog: &ToolCatalog,
    subjects: &mut Vec<InstallSubject>,
    blocking: &mut Vec<String>,
) {
    match validator_command_specs(command) {
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
                return;
            }
            for spec in specs {
                match validator_subject(&spec, catalog) {
                    Some(subject) => subjects.push(subject),
                    None => blocking.push(format!("unauditable_install_spec:{spec}")),
                }
            }
        }
    }
}

/// Classify validator preparation and run argv separately.
///
/// Keeping vectors separate prevents an install from consuming or hiding
/// the following test command. Duplicate installs and malformed vectors block.
pub(super) fn validator_command_specs(command: &ValidatorCommand) -> CommandInstalls {
    let prepare = classify_argv(&command.prepare_argv);
    if !command.prepare_argv.is_empty() && !matches!(&prepare, MiseVectors::Install(_)) {
        return CommandInstalls::Unclassifiable("prepare_without_install".to_owned());
    }
    let run = classify_argv(&command.argv);
    match (prepare, run) {
        (MiseVectors::Unclassifiable(detail), _) | (_, MiseVectors::Unclassifiable(detail)) => {
            CommandInstalls::Unclassifiable(detail)
        }
        (MiseVectors::Install(specs), MiseVectors::NotInstall) => CommandInstalls::Specs(specs),
        (MiseVectors::NotInstall, MiseVectors::Install(specs)) => CommandInstalls::Specs(specs),
        (MiseVectors::Install(_), MiseVectors::Install(_)) => {
            CommandInstalls::Unclassifiable("multiple_install_argv".to_owned())
        }
        (MiseVectors::NotInstall, MiseVectors::NotInstall) => CommandInstalls::NotInstall,
    }
}

/// Classify every Mise vector in one fixed argv, including inline scripts.
fn classify_argv(argv: &[String]) -> MiseVectors {
    let mut tokens = Vec::new();
    for arg in argv {
        if looks_like_script(arg) {
            tokens.extend(arg.split_whitespace().map(str::to_owned));
        } else {
            tokens.push(arg.clone());
        }
    }
    let words: Vec<&str> = tokens.iter().map(String::as_str).collect();
    classify_mise_vectors(&words)
}

/// True for argv elements holding a shell script rather than one token.
fn looks_like_script(arg: &str) -> bool {
    arg.chars().any(char::is_whitespace)
}

/// Classify every Mise vector in order; only one install vector is supported.
pub(super) fn classify_mise_vectors(tokens: &[&str]) -> MiseVectors {
    let mut install_specs = None;
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
            "install" => {
                if install_specs.is_some() {
                    return MiseVectors::Unclassifiable("multiple_install_vectors".to_owned());
                }
                install_specs = Some(specs_until_metachar(&tokens[cursor + 1..]));
            }
            "exec" => {}
            other => {
                return MiseVectors::Unclassifiable(format!("unexpected_subcommand:{other}"));
            }
        }
    }
    install_specs.map_or(MiseVectors::NotInstall, MiseVectors::Install)
}

/// Specs until the first shell metacharacter or argv end.
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

/// Resolve one emitted spec through the catalog or validator-specific pin.
pub(super) fn validator_subject(spec: &str, catalog: &ToolCatalog) -> Option<InstallSubject> {
    subject_for_install_spec(spec, catalog).or_else(|| {
        let (name, version) = validator_install_pin(spec)?;
        Some(InstallSubject {
            display: format!("{name}@{version}"),
            expected_version: version.to_owned(),
            lock_key: name.to_owned(),
        })
    })
}
