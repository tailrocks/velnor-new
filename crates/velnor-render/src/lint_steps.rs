// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{DenyCheck, LintCommand, RepoPath};

use crate::rules::{RenderedStep, Value};

pub(crate) fn lint_step(command: &LintCommand) -> RenderedStep {
    match command {
        LintCommand::Jactionlint { config, shellcheck } => {
            let mut words = vec!["jactionlint".to_owned()];
            words.extend(config.iter().map(|path| format!("--config-file {path}")));
            words.extend(
                [
                    "--format github",
                    "--strict-exit",
                    "--no-online",
                    "--no-color",
                    "--pyflakes=",
                ]
                .map(str::to_owned),
            );
            words.push(
                if *shellcheck {
                    "--shellcheck=shellcheck"
                } else {
                    "--shellcheck="
                }
                .to_owned(),
            );
            RenderedStep {
                name: "Lint the workflows".to_owned(),
                run: Some(words.join(" ")),
                ..RenderedStep::default()
            }
        }
        LintCommand::Alint { config } => RenderedStep {
            name: "Run alint".to_owned(),
            run: Some(format!(
                "alint --format github --config {config} --fail-on-warning check ."
            )),
            ..RenderedStep::default()
        },
        LintCommand::Probe(script) => RenderedStep {
            name: format!("Run {script}"),
            run: Some(format!("bash {script}")),
            ..RenderedStep::default()
        },
        LintCommand::MiseTask(task) => RenderedStep {
            name: format!("Run the mise task {task}"),
            run: Some(format!("mise run --force --skip-tools {task}")),
            ..RenderedStep::default()
        },
    }
}

pub(crate) fn deny(
    manifest: &RepoPath,
    config: Option<&RepoPath>,
    checks: Option<&[DenyCheck]>,
) -> (String, String, Vec<(String, Value)>) {
    let directory = manifest.parent();
    let name = directory.as_ref().map_or_else(
        || "Dependency policy".to_owned(),
        |directory| format!("Dependency policy ({directory})"),
    );
    let manifest_arg = directory.map_or_else(String::new, |_| {
        format!(
            " --manifest-path '{}'",
            manifest.as_str().replace('\'', "'\\''")
        )
    });
    let config_arg = config.map_or_else(String::new, |path| {
        format!(" --config '{}'", path.as_str().replace('\'', "'\\''"))
    });
    let checks_arg: String = checks
        .unwrap_or_default()
        .iter()
        .map(|check| [" ", check.name()].concat())
        .collect();
    (
        name,
        format!("cargo-deny{manifest_arg}{config_arg} --locked check{checks_arg}"),
        vec![],
    )
}
