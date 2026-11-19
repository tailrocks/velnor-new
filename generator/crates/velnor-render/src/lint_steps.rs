// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::LintCommand;

use crate::rules::{RenderedStep, Value};

pub(crate) const ALINT_ACTION: &str = "asamarts/alint";
const ALINT_COMMIT: &str = "5054852d00d47affc9ea71440240147d9999f4d3";
const ALINT_TAG: &str = "v0.22.1";

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
            uses: Some(format!("{ALINT_ACTION}@{ALINT_COMMIT}")),
            uses_comment: Some(ALINT_TAG.to_owned()),
            with: vec![
                ("config".to_owned(), Value::Text(config.to_string())),
                ("fail-on-warning".to_owned(), Value::Bool(true)),
                ("format".to_owned(), Value::Text("github".to_owned())),
                ("path".to_owned(), Value::Text(".".to_owned())),
                ("version".to_owned(), Value::Text(ALINT_TAG.to_owned())),
            ],
            ..RenderedStep::default()
        },
        LintCommand::Probe(script) => RenderedStep {
            name: format!("Run {script}"),
            run: Some(format!("bash {script}")),
            ..RenderedStep::default()
        },
        LintCommand::MiseTask(task) => RenderedStep {
            name: format!("Run the mise task {task}"),
            run: Some(format!("mise run --skip-tools {task}")),
            ..RenderedStep::default()
        },
    }
}
