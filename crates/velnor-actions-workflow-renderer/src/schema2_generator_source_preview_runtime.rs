// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, yaml::Yaml};
use velnor_actions_contract::{JobTimeout, ReleaseTarget};

use super::super::features::{base, finish};
use super::{ProductReleasePins, workflow_steps as steps};

pub(super) const PATH: &str = ".github/workflows/source-preview-runtime.yml";

const OUTPUTS: &[&str] = &[
    "linux_artifact_id",
    "linux_archive_digest",
    "linux_sha256",
    "darwin_artifact_id",
    "darwin_archive_digest",
    "darwin_sha256",
];
const INPUTS: &[&str] = &["public_source", "public_run_id", "public_run_attempt"];

pub(super) fn document(pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    Ok(reusable_document(
        "Verify source preview runtime",
        INPUTS,
        OUTPUTS,
        runtime_job(pins)?,
    ))
}

fn runtime_job(pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    let job_outputs = [
        (
            "linux_artifact_id",
            "${{ steps.upload_linux.outputs.artifact-id }}",
        ),
        (
            "linux_archive_digest",
            "${{ steps.upload_linux.outputs.artifact-digest }}",
        ),
        ("linux_sha256", "${{ steps.verify.outputs.linux_sha256 }}"),
        (
            "darwin_artifact_id",
            "${{ steps.upload_darwin.outputs.artifact-id }}",
        ),
        (
            "darwin_archive_digest",
            "${{ steps.upload_darwin.outputs.artifact-digest }}",
        ),
        ("darwin_sha256", "${{ steps.verify.outputs.darwin_sha256 }}"),
    ];
    let mut fields = steps::with_permissions(
        base(
            "Verify runtime SDK",
            Yaml::str("ubuntu-26.04"),
            JobTimeout::PRODUCT_COMPILER,
        ),
        steps::perm(&[("actions", "read"), ("contents", "read")]),
    );
    fields.push((
        "outputs".into(),
        Yaml::Map(
            job_outputs
                .iter()
                .map(|(key, value)| ((*key).into(), Yaml::str(*value)))
                .collect(),
        ),
    ));
    let mut setup = super::product_setup_steps(pins, ReleaseTarget::LinuxX86_64)?;
    let mut verify = steps::bash_step_with_env(
        "Verify protected runtime SDK",
        &script(pins),
        vec![
            ("GH_TOKEN", "${{ github.token }}"),
            ("PUBLIC_SOURCE", "${{ inputs.public_source }}"),
            ("PUBLIC_RUN_ID", "${{ inputs.public_run_id }}"),
            ("PUBLIC_RUN_ATTEMPT", "${{ inputs.public_run_attempt }}"),
        ],
    );
    if let Yaml::Map(fields) = &mut verify {
        fields.push(("id".into(), Yaml::str("verify")));
    }
    setup.push(verify);
    setup.push(steps::upload_step_with_id(
        "upload_linux",
        "Retain Linux SDK",
        "verified-runtime-sdk-linux-${{ github.run_attempt }}",
        &["${{ runner.temp }}/source-preview-runtime/linux"],
    ));
    setup.push(steps::upload_step_with_id(
        "upload_darwin",
        "Retain Darwin SDK",
        "verified-runtime-sdk-darwin-${{ github.run_attempt }}",
        &["${{ runner.temp }}/source-preview-runtime/darwin"],
    ));
    Ok(finish("verify", fields, setup))
}

pub(super) fn reusable_document(
    name: &str,
    inputs: &[&str],
    outputs: &[&str],
    job: (String, Yaml),
) -> Yaml {
    let inputs = inputs
        .iter()
        .map(|name| {
            (
                (*name).to_owned(),
                Yaml::Map(vec![
                    ("required".into(), Yaml::Bool(true)),
                    ("type".into(), Yaml::str("string")),
                ]),
            )
        })
        .collect();
    let outputs = outputs
        .iter()
        .map(|name| {
            (
                (*name).to_owned(),
                Yaml::Map(vec![(
                    "value".into(),
                    Yaml::str(format!("${{{{ jobs.verify.outputs.{name} }}}}")),
                )]),
            )
        })
        .collect();
    Yaml::Map(vec![
        ("name".into(), Yaml::str(name)),
        (
            "on".into(),
            Yaml::Map(vec![(
                "workflow_call".into(),
                Yaml::Map(vec![
                    ("inputs".into(), Yaml::Map(inputs)),
                    ("outputs".into(), Yaml::Map(outputs)),
                ]),
            )]),
        ),
        (
            "permissions".into(),
            steps::perm(&[("actions", "read"), ("contents", "read")]),
        ),
        ("jobs".into(), Yaml::Map(vec![job])),
    ])
}

pub(super) fn script(pins: &ProductReleasePins) -> String {
    let bootstrap = velnor_actions_mise::source_preview::BOOTSTRAP;
    let stage = velnor_actions_mise::source_preview::STAGE_VERIFIER;
    let runtime = velnor_actions_mise::source_preview::RUNTIME_VERIFIER;
    format!(
        r#"set -eu
mkdir -m 700 "$RUNNER_TEMP/source-preview-support"
cat > "$RUNNER_TEMP/source-preview-support/source_preview_bootstrap.py" <<'BOOTSTRAP'
{bootstrap}
BOOTSTRAP
cat > "$RUNNER_TEMP/source-preview-support/source_preview_stage_verify.py" <<'STAGE'
{stage}
STAGE
cat > "$RUNNER_TEMP/source-preview-support/source_preview_runtime.py" <<'RUNTIME'
{runtime}
RUNTIME
PREVIEW_GH="$(mise which gh --tool=gh@{})"
result="$(env -i PATH=/usr/bin:/bin HOME="$HOME" GH_TOKEN="$GH_TOKEN" RUNNER_TEMP="$RUNNER_TEMP" /usr/bin/python3 -I -S -c 'import runpy,sys;sys.path.insert(0,sys.argv.pop(1));runpy.run_module("source_preview_runtime",run_name="__main__")' "$RUNNER_TEMP/source-preview-support" "$PUBLIC_SOURCE" "$PUBLIC_RUN_ID" "$PUBLIC_RUN_ATTEMPT" "$PREVIEW_GH" "$RUNNER_TEMP")"
printf '%s' "$result" | /usr/bin/python3 -I -S -c 'import json,os,re,stat,sys; v=json.load(sys.stdin); assert set(v)=={{"directory","linux_sha256","darwin_sha256"}}; assert v["directory"]==os.environ["RUNNER_TEMP"]+"/source-preview-runtime"; assert all(re.fullmatch("[0-9a-f]{{64}}",v[k]) for k in ("linux_sha256","darwin_sha256")); fd=os.open(os.environ["GITHUB_OUTPUT"],os.O_WRONLY|os.O_APPEND|os.O_NOFOLLOW); s=os.fstat(fd); assert stat.S_ISREG(s.st_mode) and s.st_uid==os.geteuid() and s.st_nlink==1 and s.st_size==0; data=("linux_sha256="+v["linux_sha256"]+"\n"+"darwin_sha256="+v["darwin_sha256"]+"\n").encode(); assert os.write(fd,data)==len(data); os.close(fd)'
"#,
        pins.gh_version
    )
}
