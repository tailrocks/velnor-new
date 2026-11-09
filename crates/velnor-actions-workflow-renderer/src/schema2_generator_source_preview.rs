// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, yaml::Yaml};
use velnor_actions_contract::{JobTimeout, ReleaseTarget};

use super::super::features::{base, finish};
use super::{ProductReleasePins, assets, workflow_steps as steps};

const MAIN_PUSH: &str = "${{ github.event_name == 'push' && github.repository == 'tailrocks/velnor-new' && github.ref == 'refs/heads/main' && github.workflow_ref == 'tailrocks/velnor-new/.github/workflows/product-release-generator.yml@refs/heads/main' }}";
const TAG_PUSH: &str = "${{ github.event_name == 'push' && github.repository == 'tailrocks/velnor-new' && startsWith(github.ref, 'refs/tags/sdk-preview-') }}";
const PAYLOAD: &str = "velnor-actions-x86_64-unknown-linux-gnu";
const METADATA: &str = "${{ runner.temp }}/source-preview-metadata.json";
pub(super) const AUTHORITY_PATH: &str = ".github/workflows/source-preview-publication.yml";
const SOURCE_PREVIEW_AUTHORITY_SHA: Option<&str> =
    Some(crate::workflow_timeout::SOURCE_PREVIEW_AUTHORITY_SHA);

pub(super) fn producer_jobs(pins: &ProductReleasePins) -> Result<Vec<(String, Yaml)>, RenderError> {
    Ok(vec![build_job(pins)?, qualify_job(pins)?])
}

pub(super) fn caller_job() -> Result<Option<(String, Yaml)>, RenderError> {
    let Some(sha) = SOURCE_PREVIEW_AUTHORITY_SHA else {
        return Ok(None);
    };
    if sha.len() != 40
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(RenderError::InvalidWorkflow(
            "source_preview_authority_pin".to_owned(),
        ));
    }
    Ok(Some((
        "source-preview-publication".to_owned(),
        Yaml::Map(vec![
            ("name".to_owned(), Yaml::str("Source preview publication")),
            ("if".to_owned(), Yaml::str(TAG_PUSH)),
            (
                "uses".to_owned(),
                Yaml::uses(format!("tailrocks/velnor-new/{AUTHORITY_PATH}@{sha}")),
            ),
            (
                "permissions".to_owned(),
                steps::perm(super::preview_permissions::preview_permissions(
                    super::preview_permissions::PreviewPermissionRole::Caller,
                )),
            ),
        ]),
    )))
}

pub(super) fn authority_document(pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    let mut jobs = vec![source_job()];
    jobs.extend(super::source_preview_publication::jobs(pins)?);
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Source preview publication authority"),
        ),
        (
            "on".to_owned(),
            Yaml::Map(vec![("workflow_call".to_owned(), Yaml::Map(vec![]))]),
        ),
        (
            "permissions".to_owned(),
            steps::perm(&[("contents", "read")]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ]))
}

pub(super) fn authority_env() -> Vec<(&'static str, &'static str)> {
    vec![
        ("AUTHORITY_REPOSITORY", "${{ job.workflow_repository }}"),
        ("AUTHORITY_PATH", "${{ job.workflow_file_path }}"),
        ("AUTHORITY_REF", "${{ job.workflow_ref }}"),
        ("AUTHORITY_SHA", "${{ job.workflow_sha }}"),
    ]
}

fn fields(
    name: &str,
    needs: &[&str],
    permissions: Yaml,
    condition: &str,
    timeout: JobTimeout,
) -> Vec<(String, Yaml)> {
    let mut fields =
        steps::with_permissions(base(name, Yaml::str("ubuntu-26.04"), timeout), permissions);
    if !needs.is_empty() {
        fields = steps::with_needs(fields, needs);
    }
    fields.push(("if".to_owned(), Yaml::str(condition)));
    fields
}

fn upload(id: &str, artifact: &str, files: &[&str]) -> Yaml {
    steps::upload_step_with_id(id, "Retain exact source preview bytes", artifact, files)
}

fn artifact_output() -> (String, Yaml) {
    (
        "outputs".to_owned(),
        Yaml::Map(vec![(
            "artifact_id".to_owned(),
            Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
        )]),
    )
}

fn source_job() -> (String, Yaml) {
    let script = format!(
        "{}\n{}\n{}\n{}",
        include_str!("schema2_generator_source_preview_artifact.py"),
        include_str!("schema2_generator_source_preview_tag.py"),
        include_str!("schema2_generator_source_preview_authority.py"),
        include_str!("schema2_generator_source_preview_source.py")
    );
    let run = format!(
        "set -eu\nenv -i PATH=/usr/bin:/bin GH_TOKEN=\"$GH_TOKEN\" GITHUB_SHA=\"$GITHUB_SHA\" GITHUB_REPOSITORY=\"$GITHUB_REPOSITORY\" GITHUB_EVENT_NAME=\"$GITHUB_EVENT_NAME\" GITHUB_REF=\"$GITHUB_REF\" GITHUB_WORKFLOW_REF=\"$GITHUB_WORKFLOW_REF\" GITHUB_WORKFLOW_SHA=\"$GITHUB_WORKFLOW_SHA\" GITHUB_ACTOR_ID=\"$GITHUB_ACTOR_ID\" GITHUB_TRIGGERING_ACTOR=\"$GITHUB_TRIGGERING_ACTOR\" GITHUB_RUN_ID=\"$GITHUB_RUN_ID\" GITHUB_RUN_ATTEMPT=\"$GITHUB_RUN_ATTEMPT\" RUNNER_TEMP=\"$RUNNER_TEMP\" GITHUB_OUTPUT=\"$GITHUB_OUTPUT\" AUTHORITY_REPOSITORY=\"$AUTHORITY_REPOSITORY\" AUTHORITY_PATH=\"$AUTHORITY_PATH\" AUTHORITY_REF=\"$AUTHORITY_REF\" AUTHORITY_SHA=\"$AUTHORITY_SHA\" /usr/bin/python3 -I -S - <<'PY'\n{script}\nPY",
    );
    let mut fields = fields(
        "Admit source preview",
        &[],
        steps::perm(&[("contents", "read"), ("actions", "read")]),
        TAG_PUSH,
        JobTimeout::DEFAULT,
    );
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![
            (
                "source_artifact_id".to_owned(),
                Yaml::str("${{ steps.upload-source.outputs.artifact-id }}"),
            ),
            (
                "payload_artifact_id".to_owned(),
                Yaml::str("${{ steps.upload-payload.outputs.artifact-id }}"),
            ),
        ]),
    ));
    finish(
        "source-preview-source",
        fields,
        vec![
            steps::bash_step_with_env("Admit annotated SDK preview tag", &run, {
                let mut env = authority_env();
                env.push(("GH_TOKEN", "${{ github.token }}"));
                env
            }),
            upload(
                "upload-source",
                "source-preview-source-${{ github.run_attempt }}",
                &[METADATA],
            ),
            upload(
                "upload-payload",
                "source-preview-qualified-payload-${{ github.run_attempt }}",
                &["${{ runner.temp }}/velnor-actions-x86_64-unknown-linux-gnu"],
            ),
        ],
    )
}

fn build_job(pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    let mut action_steps = vec![
        steps::checkout_step(),
        steps::bash_step(
            "Verify protected main producer source",
            "set -eu\ntest \"$GITHUB_EVENT_NAME\" = push\ntest \"$GITHUB_REF\" = refs/heads/main\ntest \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"\ntest \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"",
        ),
    ];
    action_steps.extend(assets::build_steps(
        assets::LINUX,
        ReleaseTarget::LinuxX86_64,
        "Verify preview ELF architecture",
        &assets::linux_verify(assets::LINUX.binary),
        pins,
    )?);
    action_steps.push(steps::bash_step(
        "Name raw preview payload",
        &format!(
            "set -eu\ntest \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"\ncp '{}' '{PAYLOAD}'",
            assets::LINUX.binary,
        ),
    ));
    action_steps.push(upload(
        "upload",
        "source-preview-payload-${{ github.run_attempt }}",
        &[PAYLOAD],
    ));
    let mut fields = fields(
        "Build source preview",
        &[],
        steps::perm(&[("contents", "read")]),
        MAIN_PUSH,
        JobTimeout::PRODUCT_COMPILER,
    );
    fields.push((
        "env".to_owned(),
        Yaml::Map(steps::release_build_job_env(ReleaseTarget::LinuxX86_64)),
    ));
    fields.push(artifact_output());
    Ok(finish("source-preview-build", fields, action_steps))
}

fn qualify_job(pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    let mut action_steps = vec![
        steps::checkout_step(),
        steps::bash_step(
            "Prepare private source cache",
            "set -eu\ncase \"$RUNNER_TEMP\" in /*) ;; *) exit 1 ;; esac\nmkdir -p \"$RUNNER_TEMP/velnor\"\nrm -rf -- \"$RUNNER_TEMP/velnor/cargo\"\nmkdir -m 700 \"$RUNNER_TEMP/velnor/cargo\"",
        ),
    ];
    action_steps.extend(super::super::source_qualification::sources_restore_steps(
        "source-preview-qualify",
        "ubuntu-26.04",
    )?);
    action_steps.extend(vec![
        steps::mise_step(pins.setup_for(ReleaseTarget::LinuxX86_64))?,
        steps::command_step(
            "Install pinned preview qualification tools",
            &pins.install_qualify_tools_argv,
        )?,
        steps::verify_rust_step(pins)?,
        steps::download_step_by_id(
            "Download exact source preview payload",
            "${{ needs.source-preview-build.outputs.artifact_id }}",
            "source-preview",
        ),
        steps::bash_step_with_env(
            "Qualify exact source preview bytes",
            &qualification_script(pins),
            vec![
                (
                    "SOURCES_CACHE_HIT",
                    "${{ steps.sources-restore.outputs.cache-hit }}",
                ),
                (
                    "SOURCES_CACHE_MATCHED_KEY",
                    "${{ steps.sources-restore.outputs.cache-matched-key }}",
                ),
            ],
        ),
    ]);
    Ok(finish(
        "source-preview-qualify",
        fields(
            "Qualify source preview",
            &["source-preview-build"],
            steps::perm(&[("contents", "read")]),
            MAIN_PUSH,
            JobTimeout::PRODUCT_QUALIFICATION,
        ),
        action_steps,
    ))
}

fn qualification_script(pins: &ProductReleasePins) -> String {
    format!(
        "set -eu\nunset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN GH_TOKEN MISE_GITHUB_TOKEN GH_HOST GH_CONFIG_DIR\ntest \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"\ntest \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"\nCARGO_HOME=\"$RUNNER_TEMP/velnor/cargo\"\nexport CARGO_HOME\ntest -d \"$CARGO_HOME\"\ntest ! -L \"$CARGO_HOME\"\ntest \"$(cd \"$CARGO_HOME\" && pwd -P)\" = \"$CARGO_HOME\"\nmkdir -p \"$RUNNER_TEMP/source-qualification-artifacts/logs\"\nexport VELNOR_RUST_VERSION='{}' VELNOR_PREFETCH_RUST_ONLY=1\nbash scripts/generator-release/source-qualification.sh prefetch-locked-sources-for-qualification-workspaces\nfor manifest in Cargo.toml crates/velnor-runner/Cargo.toml; do\n  mise --no-config --no-env --no-hooks exec 'rust@{}' -- cargo metadata --locked --offline --all-features --format-version=1 --manifest-path \"$GITHUB_WORKSPACE/$manifest\" > /dev/null\ndone\nbash scripts/build-owned-archive-guard.sh --source-cache \"$CARGO_HOME\"\ntest -f source-preview/{PAYLOAD}\ntest ! -L source-preview/{PAYLOAD}\nchmod 0500 source-preview/{PAYLOAD}\ntest \"$(source-preview/{PAYLOAD} --version)\" = 'velnor-actions {}'\nguard=\"$GITHUB_WORKSPACE/.velnor/archive-guard/bin/velnor-archive-guard\"\nguard_sha256=\"$(sha256sum \"$guard\" | cut -d ' ' -f 1)\"\nscripts/capture-opentofu-goldens.sh --archive-guard \"$guard\" --archive-guard-sha256 \"$guard_sha256\" check \"$GITHUB_WORKSPACE/source-preview/{PAYLOAD}\"",
        pins.rust_version,
        pins.rust_version,
        assets::VERSION,
    )
}
