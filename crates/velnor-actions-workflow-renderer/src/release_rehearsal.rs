// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, render::RenderedFile, yaml::Yaml};
use velnor_actions_contract::config::ReleaseRehearsalConfig;

const PATH: &str = ".github/workflows/release-rehearsal.yml";

fn map(fields: Vec<(&str, Yaml)>) -> Yaml {
    Yaml::Map(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}
fn step(name: &str, script: &str, env: Yaml) -> Yaml {
    map(vec![
        ("name", Yaml::str(name)),
        ("shell", Yaml::str("bash")),
        ("env", env),
        ("run", Yaml::str(script)),
    ])
}
fn policy_env(config: &ReleaseRehearsalConfig, approved: bool) -> Yaml {
    let mut env = vec![
        ("GH_TOKEN", Yaml::str("${{ github.token }}")),
        (
            "VELNOR_DEFAULT_BRANCH",
            Yaml::str("${{ github.event.repository.default_branch }}"),
        ),
        (
            "VELNOR_REHEARSAL_ENVIRONMENT",
            Yaml::str(&config.environment),
        ),
        (
            "VELNOR_REVIEWER_ID",
            Yaml::str(config.reviewer_id.to_string()),
        ),
    ];
    if approved {
        env.push(("VELNOR_REHEARSAL_STAGE", Yaml::str("approved")));
    }
    map(env)
}
fn base_job(steps: Vec<Yaml>, permissions: Yaml) -> Vec<(&'static str, Yaml)> {
    vec![
        ("runs-on", Yaml::str("ubuntu-24.04")),
        ("timeout-minutes", Yaml::Int(45)),
        ("permissions", permissions),
        ("steps", Yaml::Seq(steps)),
    ]
}
fn read_permissions() -> Yaml {
    map(vec![
        ("actions", Yaml::str("read")),
        ("contents", Yaml::str("read")),
    ])
}
fn run_env(config: &ReleaseRehearsalConfig) -> Yaml {
    map(vec![
        ("VELNOR_REHEARSAL_SCRIPT", Yaml::str(&config.script)),
        ("CANDIDATE_PR", Yaml::str("${{ inputs.candidate_pr }}")),
        ("CANDIDATE_SHA", Yaml::str("${{ inputs.candidate_sha }}")),
    ])
}
fn preflight_job(config: &ReleaseRehearsalConfig) -> Yaml {
    let policy = include_str!("release_rehearsal_policy.sh");
    map(base_job(
        vec![step(
            "Verify environment protection",
            policy,
            policy_env(config, false),
        )],
        read_permissions(),
    ))
}

fn gate_job(config: &ReleaseRehearsalConfig) -> Yaml {
    let checkout = map(vec![
        (
            "uses",
            Yaml::str(velnor_actions_actionlint::actions::CHECKOUT_ACTION_USES),
        ),
        (
            "with",
            map(vec![
                ("ref", Yaml::str("${{ github.sha }}")),
                ("path", Yaml::str("policy")),
                ("persist-credentials", Yaml::str("false")),
            ]),
        ),
    ]);
    let prepare = map(vec![
        ("id", Yaml::str("source")),
        ("name", Yaml::str("Bind reviewed source")),
        ("shell", Yaml::str("bash")),
        ("env", run_env(config)),
        (
            "run",
            Yaml::str(include_str!("release_rehearsal_prepare.sh").replace(
                "__VELNOR_ARCHIVE_PYTHON__",
                &include_str!("release_rehearsal_archive.py").replace(
                    "__VELNOR_PATHS_PYTHON__",
                    include_str!("release_rehearsal_paths.py"),
                ),
            )),
        ),
    ]);
    let upload = map(vec![
        (
            "uses",
            Yaml::str(velnor_actions_actionlint::actions::UPLOAD_ARTIFACT_ACTION_USES),
        ),
        (
            "with",
            map(vec![
                ("name", Yaml::str("rehearsal-source")),
                ("path", Yaml::str("${{ runner.temp }}/rehearsal-source.tar")),
                ("if-no-files-found", Yaml::str("error")),
                ("retention-days", Yaml::Int(1)),
            ]),
        ),
    ]);
    let mut gate = base_job(
        vec![
            step(
                "Recheck protection and approval",
                include_str!("release_rehearsal_policy.sh"),
                policy_env(config, true),
            ),
            checkout,
            prepare,
            upload,
        ],
        read_permissions(),
    );
    gate.extend([
        ("needs", Yaml::str("preflight")),
        ("environment", Yaml::str(&config.environment)),
        (
            "outputs",
            map(vec![(
                "source_sha256",
                Yaml::str("${{ steps.source.outputs.source_sha256 }}"),
            )]),
        ),
    ]);
    map(gate)
}

fn execute_job(config: &ReleaseRehearsalConfig) -> Yaml {
    let download = map(vec![
        (
            "uses",
            Yaml::str(velnor_actions_actionlint::actions::DOWNLOAD_ARTIFACT_ACTION_USES),
        ),
        (
            "with",
            map(vec![
                ("name", Yaml::str("rehearsal-source")),
                ("path", Yaml::str("${{ runner.temp }}/rehearsal")),
            ]),
        ),
    ]);
    let Yaml::Map(mut execute_env) = run_env(config) else {
        unreachable!()
    };
    execute_env.push((
        "VELNOR_SOURCE_SHA256".to_owned(),
        Yaml::str("${{ needs.gate.outputs.source_sha256 }}"),
    ));
    let mut execute = base_job(
        vec![
            download,
            step(
                "Run rehearsal without credentials",
                &include_str!("release_rehearsal_execute.sh").replace(
                    "__VELNOR_EXTRACT_PYTHON__",
                    &include_str!("release_rehearsal_extract.py").replace(
                        "__VELNOR_PATHS_PYTHON__",
                        include_str!("release_rehearsal_paths.py"),
                    ),
                ),
                Yaml::Map(execute_env),
            ),
        ],
        map(vec![]),
    );
    execute.push(("needs", Yaml::str("gate")));
    map(execute)
}

fn witness_job(config: &ReleaseRehearsalConfig) -> Yaml {
    let Yaml::Map(mut env) = run_env(config) else {
        unreachable!()
    };
    env.push((
        "VELNOR_SOURCE_SHA256".to_owned(),
        Yaml::str("${{ needs.gate.outputs.source_sha256 }}"),
    ));
    env.push((
        "VELNOR_REHEARSAL_ENVIRONMENT".to_owned(),
        Yaml::str(&config.environment),
    ));
    env.push((
        "VELNOR_REVIEWER_ID".to_owned(),
        Yaml::str(config.reviewer_id.to_string()),
    ));
    let upload = map(vec![
        (
            "uses",
            Yaml::str(velnor_actions_actionlint::actions::UPLOAD_ARTIFACT_ACTION_USES),
        ),
        (
            "with",
            map(vec![
                ("name", Yaml::str("rehearsal-observation")),
                (
                    "path",
                    Yaml::str("${{ runner.temp }}/rehearsal-observation.json"),
                ),
                ("if-no-files-found", Yaml::str("error")),
                ("retention-days", Yaml::Int(1)),
            ]),
        ),
    ]);
    let mut job = base_job(
        vec![
            step(
                "Record runner observation",
                include_str!("release_rehearsal_witness.sh"),
                Yaml::Map(env),
            ),
            upload,
        ],
        map(vec![]),
    );
    job.push((
        "needs",
        Yaml::Seq(vec![Yaml::str("gate"), Yaml::str("execute")]),
    ));
    map(job)
}

pub fn render(config: &ReleaseRehearsalConfig, version: &str) -> Result<RenderedFile, RenderError> {
    config
        .validate("rehearsal")
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let input = || {
        map(vec![
            ("required", Yaml::Bool(true)),
            ("type", Yaml::str("string")),
        ])
    };
    let document = map(vec![
        ("name", Yaml::str("Release rehearsal")),
        (
            "on",
            map(vec![(
                "workflow_dispatch",
                map(vec![(
                    "inputs",
                    map(vec![("candidate_pr", input()), ("candidate_sha", input())]),
                )]),
            )]),
        ),
        ("permissions", map(vec![])),
        (
            "jobs",
            map(vec![
                ("preflight", preflight_job(config)),
                ("gate", gate_job(config)),
                ("execute", execute_job(config)),
                ("witness", witness_job(config)),
            ]),
        ),
    ]);
    Ok(RenderedFile {
        path: PATH.to_owned(),
        bytes: crate::render::fallback::render_checked_workflow(PATH, &document, version)?,
    })
}
