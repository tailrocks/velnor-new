// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::artifact_helpers::{document, perm};
use super::{
    BuildJob, CHECKOUT_USES, JobTimeout, Publish, ReleaseTarget, RenderError,
    Schema2WorkflowRequest, Yaml, attest_job, base, build_job, finish, generator_release,
    join_argv_for_run, publish_job, run_step, runs_on_yaml,
};
use crate::schema2::{
    ci_image_fetch::{self, CandidateFamily},
    ci_image_producer::SCRUB_EXEC,
};

const FILES: &[&str] = &[
    "RUNNER_IMAGE_MANIFEST.json",
    "LICENSE",
    "NOTICE",
    "SHA256SUMS",
];
const BUILT: &str = "${{ needs.build-images.outputs.artifact_id }}";
const ATTESTED: &str = "${{ needs.attest-images.outputs.artifact_id }}";

pub(super) fn eligibility() -> String {
    let original = crate::schema2::release_eligibility::publisher_definition_source_free();
    original.replace(
        "[[ \"${GITHUB_WORKFLOW_REF-}\" == \"$repository/.github/workflows/product-release.yml@refs/heads/main\" ]] || fail 'unexpected workflow authority path or ref'\ncase \"${GITHUB_EVENT_NAME-}\" in\n  workflow_dispatch) ;;",
        "case \"${GITHUB_WORKFLOW_REF-}/${GITHUB_EVENT_NAME-}\" in\n  \"$repository/.github/workflows/product-release.yml@refs/heads/main/workflow_dispatch\"|\"$repository/.github/workflows/runner-image-release.yml@refs/heads/main/push\") ;;",
    )
}

pub(super) fn render(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let producer = producer(request, hosted.clone())?;
    let signer = attest_job(
        "attest-images",
        "Attest runner manifest",
        hosted.clone(),
        "build-images",
        BUILT,
        FILES,
    );
    let mut publisher = publish_job(
        hosted,
        &Publish {
            id: "publish-images",
            name: "Publish runner manifest",
            needs: "attest-images",
            artifact: ATTESTED,
            prefix: "runner",
            notes: "Digest-pinned runner images from ${GITHUB_SHA}.",
            files: FILES,
        },
    );
    if let Yaml::Map(fields) = &mut publisher.1
        && let Some((_, Yaml::Seq(steps))) = fields.iter_mut().find(|(key, _)| key == "steps")
    {
        steps.retain(|step| !matches!(step, Yaml::Map(entries) if entries.iter().any(|(key,value)| key == "uses" && *value == Yaml::str(CHECKOUT_USES))));
    }
    Ok(document(
        "Runner image release",
        vec![producer, signer, publisher],
    ))
}

fn producer(request: &Schema2WorkflowRequest, hosted: Yaml) -> Result<(String, Yaml), RenderError> {
    let pins = request
        .product_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("product_release_pins_missing".to_owned()))?;
    let steps = producer_steps(pins)?;
    let mut producer = build_job(BuildJob {
        id: "build-images",
        name: "Assemble published runner manifest",
        runs_on: hosted,
        timeout: JobTimeout::new(75).map_err(RenderError::Contract)?,
        steps,
        upload_name: "Upload runner manifest assets",
        artifact: "image-assets-${{ github.run_attempt }}",
        files: FILES,
    });
    if let Yaml::Map(fields) = &mut producer.1
        && let Some((_, Yaml::Map(permissions))) =
            fields.iter_mut().find(|(key, _)| key == "permissions")
    {
        permissions.push(("packages".to_owned(), Yaml::str("read")));
    }
    Ok(producer)
}

fn producer_steps(pins: &super::super::ProductReleasePins) -> Result<Vec<Yaml>, RenderError> {
    let mut acquire = crate::schema2::qualified_sdk_preview_acquire();
    if let Yaml::Map(fields) = &mut acquire {
        let Some((_, Yaml::Map(env))) = fields.iter_mut().find(|(key, _)| key == "env") else {
            return Err(RenderError::InvalidWorkflow(
                "sdk_preview_acquire_env".to_owned(),
            ));
        };
        env.push((
            "VELNOR_IMAGE_SOURCE_SHA".to_owned(),
            Yaml::str("${{ inputs.source_sha }}"),
        ));
    }
    let mut build_argv = vec![
        "bash".to_owned(),
        "scripts/ci-native-image-credential-exec.sh".to_owned(),
    ];
    build_argv.extend(pins.resource_probe_release_build_argv.clone());
    let build = join_argv_for_run(&build_argv)?;
    let script = evidence_script(pins)?;
    let mut fetch = ci_image_fetch::step(pins, CandidateFamily::Runner)?;
    if let Yaml::Map(fields) = &mut fetch
        && let Some((_, Yaml::Str(run))) = fields.iter_mut().find(|(key, _)| key == "run")
    {
        *run = run.replace(
            "\"$GITHUB_EVENT_NAME\" == push",
            "( \"$GITHUB_EVENT_NAME\" == push || \"$GITHUB_EVENT_NAME\" == workflow_dispatch )",
        );
    }
    Ok(vec![
        generator_release::mise_setup_step(pins, ReleaseTarget::LinuxX86_64)?,
        generator_release::mise_install_step(
            "Install pinned manifest tools",
            &pins.install_resource_probe_release_tools_argv,
        )?,
        generator_release::verify_rust_step(pins)?,
        acquire,
        run_step(
            "Build source manifest formatter without credentials",
            &build,
        ),
        fetch,
        token_step(
            "Authenticate published images and format runner manifest",
            &script,
        ),
    ])
}

fn token_step(name: &str, script: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "GH_TOKEN".to_owned(),
                Yaml::str("${{ github.token }}"),
            )]),
        ),
        ("run".to_owned(), Yaml::str(script)),
    ])
}

fn evidence_script(pins: &super::super::ProductReleasePins) -> Result<String, RenderError> {
    let gh = generator_release::gh_function(pins)?;
    let download = crate::schema2::SDK_CI_IMAGE_DOWNLOAD_SCRIPT.replace(
        "__VELNOR_ARTIFACT_EXTRACTOR__",
        crate::schema2::SDK_CI_IMAGE_ARTIFACT_SCRIPT,
    );
    let body = include_str!("schema2_runner_image_release.sh")
        .replace("__VERSION__", env!("CARGO_PKG_VERSION"))
        .replace(
            "__POLICY__",
            include_str!("schema2_runner_image_publication_policy.sh"),
        )
        .replace("__SCRUB_EXEC__", SCRUB_EXEC);
    Ok(format!(
        "set -euo pipefail\nrepo=tailrocks/velnor-new\nsha=\"$VELNOR_SOURCE_SHA\"\nbase=\"$RUNNER_TEMP/velnor-runner-release\"\numask 077\nmkdir -m 700 \"$base\"\n{gh}\n{}\n{download}\n{body}",
        crate::schema2::SDK_CI_IMAGE_LOOKUP_SCRIPT
    ))
}

fn caller_gate(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let pins = request
        .product_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("image_pins_missing".to_owned()))?;
    let mut fields = base(
        "Check image release source eligibility",
        runs_on_yaml(&request.hosted_label)?,
        JobTimeout::new(75).map_err(RenderError::Contract)?,
    );
    fields.push((
        "permissions".to_owned(),
        perm(&[("actions", "read"), ("contents", "read")]),
    ));
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(
            [
                "source_sha",
                "workflow_authority_sha",
                "ci_run_id",
                "ci_attempt",
            ]
            .map(|name| {
                (
                    name.to_owned(),
                    Yaml::str(format!("${{{{ steps.check.outputs.{name} }}}}")),
                )
            })
            .to_vec(),
        ),
    ));
    let script = format!(
        "set -euo pipefail\n{}\n{}\nfor ((retry=0;retry<360;retry++)); do\n if (release_eligibility); then exit 0; fi\n sleep 10\ndone\nexit 1",
        generator_release::gh_function(pins)?,
        eligibility()
    );
    let mut check = token_step("Wait for exact successful source CI", &script);
    if let Yaml::Map(parts) = &mut check {
        parts.push(("id".to_owned(), Yaml::str("check")));
    }
    let mut steps = generator_release::product_setup_steps(pins, ReleaseTarget::LinuxX86_64)?;
    steps.push(check);
    let (_, gate) = finish("release-eligibility", fields, steps);
    Ok(gate)
}

pub(super) fn caller(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let mut image_request = request.clone();
    image_request
        .workflows
        .retain(|workflow| *workflow == velnor_actions_contract::RoutingWorkflow::ImageRelease);
    let generated = crate::schema2::product_release::render(&image_request)?
        .ok_or_else(|| RenderError::InvalidWorkflow("image_release_missing".to_owned()))?;
    let mut workflow = generated.workflow;
    if let Yaml::Map(fields) = &mut workflow {
        fields.retain(|(key, _)| key != "name" && key != "on");
        fields.push((
            "name".to_owned(),
            Yaml::str("Publish runner image manifest"),
        ));
        fields.push((
            "on".to_owned(),
            Yaml::Map(vec![(
                "push".to_owned(),
                Yaml::Map(vec![(
                    "branches".to_owned(),
                    Yaml::Seq(vec![Yaml::str("main")]),
                )]),
            )]),
        ));
        if let Some((_, Yaml::Map(jobs))) = fields.iter_mut().find(|(key, _)| key == "jobs") {
            let gate = caller_gate(request)?;
            if let Some((_, job)) = jobs.iter_mut().find(|(id, _)| id == "release-eligibility") {
                *job = gate;
            }
        }
    }
    Ok(workflow)
}
