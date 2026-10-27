// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use crate::composite::{
    composite_yaml, composite_yaml_with_inputs, shared_call_named, shared_call_named_with_inputs,
};
use crate::yaml::Yaml;
use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::ReleaseTarget;

use super::super::features::{base, finish};
use super::{ProductReleasePins, assets, manifest, workflow_steps};

pub(super) fn build_job(
    id: &str,
    name: &str,
    action: &str,
    runs_on: Yaml,
    steps: Vec<Yaml>,
    product: assets::ProductAsset,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let call = local_action(action, name, steps, actions)?;
    let mut fields = workflow_steps::with_permissions(
        base(name, runs_on, JobTimeout::PRODUCT_COMPILER),
        workflow_steps::build_permissions(),
    );
    fields.retain(|(key, _)| key != "name");
    fields = workflow_steps::with_needs(fields, &["verify-release-source"]);
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![(
            "artifact_id".to_owned(),
            Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
        )]),
    ));
    fields.push((
        "env".to_owned(),
        Yaml::Map(workflow_steps::release_build_job_env(product.target)),
    ));
    Ok(finish(
        id,
        fields,
        vec![
            workflow_steps::checkout_step(),
            call,
            workflow_steps::upload_step_with_id(
                "upload",
                product.upload_name,
                product.workflow_artifact,
                &assets::artifact_upload_paths(product),
            ),
        ],
    ))
}

pub(super) fn attest_job(
    id: &str,
    name: &str,
    runs_on: Yaml,
    product: assets::ProductAsset,
    pins: &ProductReleasePins,
) -> Result<(String, Yaml), RenderError> {
    let files = [product.binary, product.sidecar, product.provenance];
    let artifact_id = format!("${{{{ needs.{}.outputs.artifact_id }}}}", product.build_job);
    let downloads =
        assets::download_build_steps_for_id(product, "Download built asset files", &artifact_id);
    let mut action_steps = vec![workflow_steps::mise_step(pins.setup_for(product.target))?];
    action_steps.push(authority_gh_validation_step());
    action_steps.push(authority_gh_install_step()?);
    action_steps.extend(downloads);
    action_steps.extend([
        workflow_steps::bash_step_with_env(
            "Verify candidate provenance record",
            &assets::verify_authority_provenance_script(product),
            authority_toolchain_env(),
        ),
        workflow_steps::bash_step(
            "Verify downloaded checksum sidecar",
            &format!(
                "set -eu\ncd {}\n{} {}",
                product.directory, product.checksum_command, product.sidecar
            ),
        ),
        workflow_steps::attest_step(&workflow_steps::subject_list(&files, product.directory)),
    ]);
    let bundle_paths = manifest::asset_attestation_bundle_paths(product);
    let bundle_path_refs = bundle_paths.iter().map(String::as_str).collect::<Vec<_>>();
    action_steps.extend([
        workflow_steps::bash_step_with_token(
            "Fetch and verify candidate attestation bundles",
            &manifest::asset_attestation_bundle_script(product),
            &authority_gh_argv(),
        )?,
        workflow_steps::upload_step(
            "Upload verified candidate attestation bundles",
            &format!("{}-attestations", product.workflow_artifact),
            &bundle_path_refs,
        ),
    ]);
    let mut fields = workflow_steps::with_needs(
        workflow_steps::with_permissions(
            base(name, runs_on, JobTimeout::DEFAULT),
            workflow_steps::attest_permissions(),
        ),
        &if product.target == ReleaseTarget::MacosArm64 {
            vec![
                product.build_job,
                "candidate-manifest",
                product.qualify_job,
                "qualify-homebrew-macos",
            ]
        } else {
            vec![product.build_job, "candidate-manifest", product.qualify_job]
        },
    );
    fields.retain(|(key, _)| key != "name");
    fields.push((
        "env".to_owned(),
        Yaml::Map(
            authority_toolchain_env()
                .into_iter()
                .map(|(key, value)| (key.to_owned(), Yaml::str(value)))
                .collect(),
        ),
    ));
    Ok(finish(id, fields, action_steps))
}

pub(super) fn publish_job(
    hosted: Yaml,
    pins: &ProductReleasePins,
) -> Result<(String, Yaml), RenderError> {
    let steps = publish_steps(pins)?;
    let mut fields = workflow_steps::with_needs(
        workflow_steps::with_permissions(
            base("Publish velnor-actions", hosted, JobTimeout::DEFAULT),
            workflow_steps::publish_permissions(),
        ),
        &[
            "attest-linux",
            "attest-macos",
            "attest-macos-intel",
            "attest-manifest",
            "candidate-manifest",
            "qualify-homebrew-macos",
            "build-linux",
            "build-macos",
            "build-macos-intel",
        ],
    );
    fields.retain(|(key, _)| key != "name");
    fields.push((
        "environment".to_owned(),
        Yaml::Map(vec![("name".to_owned(), Yaml::str("generator-release"))]),
    ));
    fields.push((
        "env".to_owned(),
        Yaml::Map(
            authority_toolchain_env()
                .into_iter()
                .map(|(key, value)| (key.to_owned(), Yaml::str(value)))
                .collect(),
        ),
    ));
    let acceptance_paths = manifest::acceptance_artifact_paths();
    let acceptance_path_refs = acceptance_paths.to_vec();
    let acceptance_name = manifest::acceptance_artifact_name();
    Ok(finish(
        "publish-generator",
        fields,
        steps
            .into_iter()
            .chain([workflow_steps::upload_step(
                "Upload verified immutable release acceptance",
                &acceptance_name,
                &acceptance_path_refs,
            )])
            .collect(),
    ))
}

fn authority_toolchain_env() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "VELNOR_RELEASE_RUST_VERSION",
            "${{ needs.candidate-manifest.outputs.rust_version }}",
        ),
        (
            "VELNOR_RELEASE_MR_BOXINGTON_VERSION",
            "${{ needs.candidate-manifest.outputs.mr_boxington_version }}",
        ),
        (
            "VELNOR_RELEASE_GH_VERSION",
            "${{ needs.candidate-manifest.outputs.gh_version }}",
        ),
    ]
}

pub(super) fn authority_gh_argv() -> Vec<String> {
    [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "gh@$VELNOR_RELEASE_GH_VERSION",
        "--",
        "gh",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub(super) fn authority_gh_install_argv() -> Vec<String> {
    [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "install",
        "gh@$VELNOR_RELEASE_GH_VERSION",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub(super) fn authority_gh_install_step() -> Result<Yaml, RenderError> {
    workflow_steps::command_step(
        "Install source-bound GitHub CLI",
        &authority_gh_install_argv(),
    )
}

pub(super) fn authority_gh_validation_step() -> Yaml {
    workflow_steps::bash_step(
        "Validate source-bound GitHub CLI version",
        "set -eu\nexact_semver='^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$'\n[[ \"${VELNOR_RELEASE_GH_VERSION:-}\" =~ $exact_semver ]] || exit 1",
    )
}

fn publish_steps(pins: &ProductReleasePins) -> Result<Vec<Yaml>, RenderError> {
    let mut steps = vec![workflow_steps::mise_step(
        pins.setup_for(ReleaseTarget::LinuxX86_64),
    )?];
    steps.push(authority_gh_validation_step());
    steps.push(authority_gh_install_step()?);
    for asset in assets::ASSETS {
        let artifact_id = format!("${{{{ needs.{}.outputs.artifact_id }}}}", asset.build_job);
        steps.extend(assets::download_build_steps_for_id(
            asset,
            "Download exact release build artifact",
            &artifact_id,
        ));
    }
    steps.push(workflow_steps::download_step_by_id(
        "Download canonical same-run release manifest",
        "${{ needs.candidate-manifest.outputs.artifact_id }}",
        manifest::DIR,
    ));
    steps.push(workflow_steps::bash_step_with_env(
        "Verify canonical manifest digest",
        &manifest::manifest_digest_check_script(),
        vec![(
            "VELNOR_RELEASE_MANIFEST_SHA256",
            "${{ needs.candidate-manifest.outputs.manifest_sha256 }}",
        )],
    ));
    for artifact in [
        format!("{}-attestations", assets::LINUX.workflow_artifact),
        format!("{}-attestations", assets::MACOS_ARM64.workflow_artifact),
        format!("{}-attestations", assets::MACOS_X86_64.workflow_artifact),
        format!("{}-attestations", manifest::ARTIFACT),
    ] {
        steps.push(workflow_steps::download_step(
            "Download verified release attestation bundles",
            &artifact,
            manifest::ATTESTATION_DIR,
        ));
    }
    steps.push(workflow_steps::bash_step_with_token(
        "Fetch and verify signed release attestations",
        &manifest::attestation_bundle_script(),
        &authority_gh_argv(),
    )?);
    steps.push(workflow_steps::publish_step(
        &manifest::publish_script(),
        &authority_gh_argv(),
    )?);
    Ok(steps)
}

pub(super) fn local_action(
    logical: &str,
    name: &str,
    steps: Vec<Yaml>,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<Yaml, RenderError> {
    let action = composite_yaml(name, steps)?;
    actions.push((format!(".github/actions/{logical}/action.yml"), action));
    shared_call_named(&format!("./.github/actions/{logical}"), name)
}

pub(super) fn local_action_with_inputs(
    logical: &str,
    name: &str,
    steps: Vec<Yaml>,
    inputs: Vec<(&str, &str, &str)>,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<Yaml, RenderError> {
    let definitions = inputs
        .iter()
        .map(|(input_name, description, _)| {
            (
                (*input_name).to_owned(),
                Yaml::Map(vec![
                    ("description".to_owned(), Yaml::str(*description)),
                    ("required".to_owned(), Yaml::Bool(true)),
                ]),
            )
        })
        .collect();
    let action = composite_yaml_with_inputs(name, definitions, steps)?;
    actions.push((format!(".github/actions/{logical}/action.yml"), action));
    shared_call_named_with_inputs(
        &format!("./.github/actions/{logical}"),
        name,
        inputs
            .into_iter()
            .map(|(input_name, _, value)| (input_name.to_owned(), Yaml::str(value)))
            .collect(),
    )
}
