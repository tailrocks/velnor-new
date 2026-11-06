// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, render::RenderedFile, yaml::Yaml};
use velnor_actions_contract::config::ReleaseRehearsalConfig;

#[path = "release_rehearsal_metadata.rs"]
mod metadata;
#[path = "release_rehearsal_output.rs"]
mod output;
#[path = "release_rehearsal_policy.rs"]
mod policy;

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
fn base_job(steps: Vec<Yaml>, permissions: Yaml) -> Vec<(&'static str, Yaml)> {
    vec![
        ("runs-on", Yaml::str("ubuntu-26.04")),
        ("timeout-minutes", Yaml::Int(45)),
        ("permissions", permissions),
        ("steps", Yaml::Seq(steps)),
    ]
}
fn run_env(config: &ReleaseRehearsalConfig) -> Yaml {
    map(vec![
        ("VELNOR_REHEARSAL_SCRIPT", Yaml::str(&config.script)),
        ("CANDIDATE_PR", Yaml::str("${{ inputs.candidate_pr }}")),
        ("CANDIDATE_SHA", Yaml::str("${{ inputs.candidate_sha }}")),
    ])
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
    let mut steps = vec![
        step(
            "Recheck protection and approval",
            include_str!("release_rehearsal_policy.sh"),
            policy::policy_env(config, true),
        ),
        checkout,
        prepare,
        upload,
    ];
    if config.metadata.is_some() {
        append_approved_source_steps(&mut steps);
    }
    let mut gate = base_job(steps, policy::read_permissions());
    gate.extend([
        ("needs", Yaml::str("preflight")),
        ("environment", Yaml::str(&config.environment)),
        ("outputs", gate_outputs(config)),
    ]);
    map(gate)
}

fn gate_outputs(config: &ReleaseRehearsalConfig) -> Yaml {
    let mut outputs = vec![(
        "source_sha256",
        Yaml::str("${{ steps.source.outputs.source_sha256 }}"),
    )];
    if config.metadata.is_some() {
        outputs.push((
            "approved_sha256",
            Yaml::str("${{ steps.approved.outputs.approved_sha256 }}"),
        ));
        outputs.push((
            "approved_tree",
            Yaml::str("${{ steps.approved.outputs.approved_tree }}"),
        ));
    }
    map(outputs)
}

fn append_approved_source_steps(steps: &mut Vec<Yaml>) {
    steps.push(step(
        "Validate approved source identity",
        "set -euo pipefail\n[[ \"$SOURCE_SHA\" =~ ^[a-f0-9]{40}$ ]]\n",
        map(vec![("SOURCE_SHA", Yaml::str("${{ inputs.source_sha }}"))]),
    ));
    steps.push(map(vec![
        (
            "uses",
            Yaml::str(velnor_actions_actionlint::actions::CHECKOUT_ACTION_USES),
        ),
        (
            "with",
            map(vec![
                ("ref", Yaml::str("${{ inputs.source_sha }}")),
                ("path", Yaml::str("approved-source")),
                ("persist-credentials", Yaml::str("false")),
            ]),
        ),
    ]));
    let archive = include_str!("release_rehearsal_archive.py").replace(
        "__VELNOR_PATHS_PYTHON__",
        include_str!("release_rehearsal_paths.py"),
    );
    let prepare = include_str!("release_rehearsal_prepare_metadata.sh")
        .replace("__VELNOR_ARCHIVE_PYTHON__", &archive);
    steps.push(map(vec![
        ("id", Yaml::str("approved")),
        ("name", Yaml::str("Bind approved source")),
        ("shell", Yaml::str("bash")),
        (
            "env",
            map(vec![("SOURCE_SHA", Yaml::str("${{ inputs.source_sha }}"))]),
        ),
        ("run", Yaml::str(prepare)),
    ]));
    steps.push(map(vec![
        (
            "uses",
            Yaml::str(velnor_actions_actionlint::actions::UPLOAD_ARTIFACT_ACTION_USES),
        ),
        (
            "with",
            map(vec![
                ("name", Yaml::str("approved-source")),
                ("path", Yaml::str("${{ runner.temp }}/approved-source.tar")),
                ("if-no-files-found", Yaml::str("error")),
                ("retention-days", Yaml::Int(1)),
            ]),
        ),
    ]));
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
    let mut steps = vec![
        download,
        step(
            "Run rehearsal without credentials",
            &include_str!("release_rehearsal_execute.sh")
                .replace(
                    "__VELNOR_OUTPUT_ENV__",
                    if config.output_bundle {
                        "VELNOR_REHEARSAL_OUTPUT=$RUNNER_TEMP/rehearsal-output.tar"
                    } else {
                        ""
                    },
                )
                .replace(
                    "__VELNOR_EXTRACT_PYTHON__",
                    &include_str!("release_rehearsal_extract.py").replace(
                        "__VELNOR_PATHS_PYTHON__",
                        include_str!("release_rehearsal_paths.py"),
                    ),
                ),
            Yaml::Map(execute_env),
        ),
    ];
    if config.output_bundle {
        steps.push(step("Validate untrusted rehearsal output", "set -euo pipefail\np=\"$RUNNER_TEMP/rehearsal-output.tar\"\ntest ! -L \"$p\" && test -f \"$p\"\ntest \"$(wc -c < \"$p\")\" -le 104857600\n", map(vec![])));
        steps.push(map(vec![
            (
                "uses",
                Yaml::str(velnor_actions_actionlint::actions::UPLOAD_ARTIFACT_ACTION_USES),
            ),
            (
                "with",
                map(vec![
                    ("name", Yaml::str("rehearsal-output-untrusted")),
                    ("path", Yaml::str("${{ runner.temp }}/rehearsal-output.tar")),
                    ("if-no-files-found", Yaml::str("error")),
                    ("retention-days", Yaml::Int(1)),
                ]),
            ),
        ]));
    }
    let mut execute = base_job(steps, map(vec![]));
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
        ("id", Yaml::str("observation_upload")),
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
    let mut steps = vec![
        step(
            "Record runner observation",
            include_str!("release_rehearsal_witness.sh"),
            Yaml::Map(env),
        ),
        upload,
        step(
            "Bind observation artifact",
            include_str!("release_rehearsal_artifact_marker.sh"),
            map(vec![
                (
                    "VELNOR_ARTIFACT_ID",
                    Yaml::str("${{ steps.observation_upload.outputs.artifact-id }}"),
                ),
                (
                    "VELNOR_ARTIFACT_DIGEST",
                    Yaml::str("${{ steps.observation_upload.outputs.artifact-digest }}"),
                ),
            ]),
        ),
    ];
    if config.output_bundle {
        output::append_output_witness_steps(&mut steps);
    }
    if let Some(metadata) = &config.metadata {
        metadata::append_metadata_witness_steps(&mut steps, metadata);
    }
    let mut job = base_job(steps, map(vec![]));
    let mut needs = vec![Yaml::str("gate"), Yaml::str("execute")];
    if config.metadata.is_some() {
        needs.push(Yaml::str("metadata"));
    }
    job.push(("needs", Yaml::Seq(needs)));
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
    let mut inputs = vec![("candidate_pr", input()), ("candidate_sha", input())];
    if config.metadata.is_some() {
        inputs.push(("source_sha", input()));
    }
    let mut jobs = vec![
        ("preflight", policy::preflight_job(config)),
        ("gate", gate_job(config)),
        ("execute", execute_job(config)),
    ];
    if let Some(metadata) = &config.metadata {
        jobs.push(("metadata", metadata::metadata_job(metadata)?));
    }
    jobs.push(("witness", witness_job(config)));
    let document = map(vec![
        ("name", Yaml::str("Release rehearsal")),
        (
            "on",
            map(vec![(
                "workflow_dispatch",
                map(vec![("inputs", map(inputs))]),
            )]),
        ),
        ("permissions", map(vec![])),
        ("jobs", map(jobs)),
    ]);
    Ok(RenderedFile {
        path: PATH.to_owned(),
        bytes: crate::render::fallback::render_checked_workflow(PATH, &document, version)?,
    })
}
