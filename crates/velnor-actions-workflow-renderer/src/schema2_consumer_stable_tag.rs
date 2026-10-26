// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::shared::{TapInvocationContext, acquire};
use super::{ConsumerReleaseSpec, ConsumerStableReleaseSpec, RenderError, Yaml, steps};
use crate::{
    commands::join_argv_for_run,
    schema2::features::{base, finish, run_step},
};
use velnor_actions_contract::JobTimeout;

pub(super) fn prepare(
    spec: &ConsumerReleaseSpec,
    stable: &ConsumerStableReleaseSpec,
) -> Result<(String, Yaml), RenderError> {
    let mut fields = authority_fields(
        "Prepare the stable version PR",
        "${{ github.event_name == 'workflow_dispatch' && inputs.operation == 'prepare' && inputs.original_run_id == '' && inputs.original_run_attempt == '' }}",
        true,
    );
    fields.push(("environment".to_owned(), Yaml::str("release")));
    let mut steps = bootstrap(spec, "stable-prepare")?;
    steps.push(configuration(stable)?);
    steps.push(metadata_checkout(spec));
    steps.push(prepare_plz(stable)?);
    Ok(finish("stable-prepare", fields, steps))
}
pub(super) fn release(
    spec: &ConsumerReleaseSpec,
    _stable: &ConsumerStableReleaseSpec,
) -> Result<(String, Yaml), RenderError> {
    let mut fields = authority_fields(
        "Authorize the permanent stable tag",
        TapInvocationContext::Stable.production_if(),
        false,
    );
    fields.push(("environment".to_owned(), Yaml::str("release")));
    let mut outputs = vec![];
    for key in [
        "source_sha",
        "workflow_authority_sha",
        "ci_run_id",
        "ci_attempt",
        "commit_count",
    ] {
        outputs.push((
            key.to_owned(),
            Yaml::str(format!("${{{{ needs.stable-eligibility.outputs.{key} }}}}")),
        ));
    }
    for (key, value) in [
        (
            "origin_artifact_id",
            "${{ steps.origin-upload.outputs.artifact-id }}",
        ),
        (
            "origin_artifact_digest",
            "${{ steps.origin-upload.outputs.artifact-digest }}",
        ),
        (
            "intent_artifact_id",
            "${{ steps.intent.outputs.original_intent_artifact_id || steps.intent-upload.outputs.artifact-id }}",
        ),
        (
            "intent_artifact_digest",
            "${{ steps.intent.outputs.original_intent_artifact_digest || steps.intent-upload.outputs.artifact-digest }}",
        ),
    ] {
        outputs.push((key.to_owned(), Yaml::str(value)));
    }
    fields.push(("outputs".to_owned(), Yaml::Map(outputs)));
    let mut steps = bootstrap(spec, "stable-tag")?;
    steps.push(native(spec, "write-stable-tag-intent-v1")?);
    steps.push(retain("intent", &["intent.json"]));
    steps.push(native(spec, "write-stable-tag-origin-v1")?);
    steps.push(retain("origin", &["origin.json"]));
    Ok(finish("stable-tag", fields, steps))
}
fn authority_fields(name: &str, condition: &str, prepare: bool) -> Vec<(String, Yaml)> {
    let mut fields = base(name, Yaml::str("ubuntu-26.04"), JobTimeout::DEFAULT);
    fields.extend([
        ("if".to_owned(), Yaml::str(condition)),
        ("needs".to_owned(), steps::needs(&["stable-eligibility"])),
        (
            "permissions".to_owned(),
            steps::permissions(if prepare {
                &[
                    ("contents", "write"),
                    ("actions", "read"),
                    ("pull-requests", "write"),
                ]
            } else {
                &[("contents", "write"), ("actions", "read")]
            }),
        ),
    ]);
    fields
}
fn bootstrap(spec: &ConsumerReleaseSpec, id: &str) -> Result<Vec<Yaml>, RenderError> {
    Ok(vec![
        steps::setup(&spec.linux_setup),
        run_step(
            "Install pinned GitHub CLI",
            &join_argv_for_run(&spec.install_gh_argv)?,
        ),
        acquire(&spec.linux_acquire, id, "ubuntu-26.04")?,
    ])
}
fn configuration(stable: &ConsumerStableReleaseSpec) -> Result<Yaml, RenderError> {
    let config = crate::release_config::stable_release_pr_config(&stable.version_package)?;
    Ok(steps::script(
        "Materialize the native version-PR release-plz configuration",
        "configuration",
        "set -euo pipefail\numask 077\nmkdir -p \"$RUNNER_TEMP/velnor/stable-tag\"\n(set -C; printf '%s' \"$VELNOR_STABLE_FIXED_CONFIG\" > \"$RUNNER_TEMP/velnor/stable-tag/release-plz.toml\")\n",
        &[("VELNOR_STABLE_FIXED_CONFIG", &config)],
    ))
}
fn metadata_checkout(spec: &ConsumerReleaseSpec) -> Yaml {
    steps::checkout(spec, "${{ needs.stable-eligibility.outputs.source_sha }}")
}
fn prepare_plz(stable: &ConsumerStableReleaseSpec) -> Result<Yaml, RenderError> {
    let prefix = join_argv_for_run(&stable.release_plz_argv)?;
    let manifest = crate::commands::quote_run_arg(&stable.manifest_path)?;
    let package = crate::commands::quote_run_arg(&stable.version_package)?;
    let run = format!(
        "set -euo pipefail\numask 077\n(ulimit -f 8192; {prefix} release-pr -p {package} --config \"$RUNNER_TEMP/velnor/stable-tag/release-plz.toml\" --manifest-path {manifest} -o json > \"$RUNNER_TEMP/velnor/stable-tag/release-plz-output.json\")\n"
    );
    Ok(steps::script(
        "Prepare the version PR with qualified release-plz",
        "release-plz",
        &run,
        &[
            ("GIT_TOKEN", "${{ github.token }}"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_COUNT", "3"),
            ("GIT_CONFIG_KEY_0", "core.hooksPath"),
            ("GIT_CONFIG_VALUE_0", "/dev/null"),
            ("GIT_CONFIG_KEY_1", "tag.gpgSign"),
            ("GIT_CONFIG_VALUE_1", "false"),
            ("GIT_CONFIG_KEY_2", "commit.gpgSign"),
            ("GIT_CONFIG_VALUE_2", "false"),
        ],
    ))
}

fn native(spec: &ConsumerReleaseSpec, op: &str) -> Result<Yaml, RenderError> {
    let mut step = steps::operation(spec, op, true, TapInvocationContext::StableAuthority)?;
    if let Yaml::Map(fields) = &mut step
        && let Some((_, value)) = fields.iter_mut().find(|(key, _)| key == "id")
    {
        *value = Yaml::str(if op == "write-stable-tag-intent-v1" {
            "intent"
        } else {
            "tag"
        });
    }
    if op == "write-stable-tag-origin-v1" {
        super::shared::production::operation_env(
            &mut step,
            &[
                (
                    "VELNOR_STABLE_TAG_INTENT_ARTIFACT_ID",
                    "${{ steps.intent.outputs.original_intent_artifact_id || steps.intent-upload.outputs.artifact-id }}",
                ),
                (
                    "VELNOR_STABLE_TAG_INTENT_ARTIFACT_DIGEST",
                    "${{ steps.intent.outputs.original_intent_artifact_digest || steps.intent-upload.outputs.artifact-digest }}",
                ),
            ],
        )?;
    }
    Ok(step)
}
fn retain(kind: &str, files: &[&str]) -> Yaml {
    let mut fields = vec![
        (
            "name".to_owned(),
            Yaml::str("Retain the immutable native stable authority"),
        ),
        ("id".to_owned(), Yaml::str(format!("{kind}-upload"))),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::UPLOAD_ARTIFACT_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                (
                    "name".to_owned(),
                    Yaml::str(format!(
                        "velnor-stable-tag-{kind}-r${{{{ github.run_id }}}}-a${{{{ github.run_attempt }}}}"
                    )),
                ),
                (
                    "path".to_owned(),
                    Yaml::str(
                        files
                            .iter()
                            .map(|file| format!("${{{{ runner.temp }}}}/velnor/stable-tag/{file}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                ),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("retention-days".to_owned(), Yaml::Int(30)),
                ("overwrite".to_owned(), Yaml::Bool(false)),
                ("include-hidden-files".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ];
    if kind == "intent" {
        fields.push((
            "if".to_owned(),
            Yaml::str("${{ steps.intent.outputs.resumed == 'false' }}"),
        ));
    }
    Yaml::Map(fields)
}
