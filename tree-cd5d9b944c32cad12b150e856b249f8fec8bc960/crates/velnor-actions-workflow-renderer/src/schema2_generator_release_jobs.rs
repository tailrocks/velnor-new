//! Build, qualify, attest, and publish workflow jobs.

use crate::RenderError;
use crate::composite::{
    composite_yaml, composite_yaml_with_inputs, shared_call_named, shared_call_named_with_inputs,
};
use crate::yaml::Yaml;
use velnor_actions_contract::ReleaseTarget;

use super::super::features::{base, finish};
use super::{GeneratorReleasePins, assets, manifest, workflow_steps};

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
        base(name, runs_on, 120),
        workflow_steps::build_permissions(),
    );
    fields.retain(|(key, _)| key != "name");
    fields = workflow_steps::with_needs(fields, &["verify-release-source"]);
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![
            (
                "artifact_id".to_owned(),
                Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
            ),
            (
                "artifact_digest".to_owned(),
                Yaml::str("${{ steps.upload.outputs.artifact-digest }}"),
            ),
        ]),
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
                &[product.archive],
            ),
        ],
    ))
}

pub(super) fn attest_job(
    id: &str,
    name: &str,
    action: &str,
    runs_on: Yaml,
    product: assets::ProductAsset,
    pins: &GeneratorReleasePins,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let files = [product.binary, product.sidecar, product.provenance];
    let downloads = assets::download_build_steps(product, "Download built asset archive");
    let mut action_steps = vec![workflow_steps::mise_step(pins.setup_for(product.target))?];
    action_steps.push(workflow_steps::install_gh_step(&pins.install_gh_argv)?);
    action_steps.extend(downloads);
    action_steps.extend([
        workflow_steps::bash_step(
            "Verify candidate provenance record",
            &assets::verify_provenance_script(product, pins),
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
            &pins.gh_argv,
        )?,
        workflow_steps::upload_step(
            "Upload verified candidate attestation bundles",
            &format!("{}-attestations", product.workflow_artifact),
            &bundle_path_refs,
        ),
    ]);
    let call = local_action_with_input(
        action,
        name,
        action_steps,
        "artifact_id",
        "Artifact ID from this target's build job",
        &format!("${{{{ needs.{}.outputs.artifact_id }}}}", product.build_job),
        actions,
    )?;
    let mut fields = workflow_steps::with_needs(
        workflow_steps::with_permissions(
            base(name, runs_on, 20),
            workflow_steps::attest_permissions(),
        ),
        &[product.build_job, product.qualify_job],
    );
    fields.retain(|(key, _)| key != "name");
    Ok(finish(
        id,
        fields,
        vec![workflow_steps::checkout_step(), call],
    ))
}

pub(super) fn publish_job(
    hosted: Yaml,
    pins: &GeneratorReleasePins,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let steps = publish_steps(pins)?;
    let call = local_action_with_inputs(
        "generator-release-publish",
        "Publish generator release",
        steps,
        publish_action_inputs(),
        actions,
    )?;
    let mut fields = workflow_steps::with_needs(
        workflow_steps::with_permissions(
            base("Publish velnor-actions", hosted, 30),
            workflow_steps::publish_permissions(),
        ),
        &[
            "attest-linux",
            "attest-macos",
            "attest-macos-intel",
            "attest-manifest",
            "candidate-manifest",
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
    let acceptance_paths = manifest::acceptance_artifact_paths();
    let acceptance_path_refs = acceptance_paths.to_vec();
    let acceptance_name = manifest::acceptance_artifact_name();
    Ok(finish(
        "publish-generator",
        fields,
        vec![
            workflow_steps::checkout_step(),
            call,
            workflow_steps::upload_step(
                "Upload verified immutable release acceptance",
                &acceptance_name,
                &acceptance_path_refs,
            ),
        ],
    ))
}

fn publish_action_inputs() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "linux_artifact_id",
            "Artifact ID from the Linux x86_64 build job",
            "${{ needs.build-linux.outputs.artifact_id }}",
        ),
        (
            "macos_arm64_artifact_id",
            "Artifact ID from the macOS arm64 build job",
            "${{ needs.build-macos.outputs.artifact_id }}",
        ),
        (
            "macos_x86_64_artifact_id",
            "Artifact ID from the macOS x86_64 build job",
            "${{ needs.build-macos-intel.outputs.artifact_id }}",
        ),
        (
            "manifest_artifact_id",
            "Artifact ID of the canonical same-run manifest",
            "${{ needs.candidate-manifest.outputs.artifact_id }}",
        ),
        (
            "manifest_sha256",
            "SHA-256 of the canonical same-run manifest bytes",
            "${{ needs.candidate-manifest.outputs.manifest_sha256 }}",
        ),
    ]
}

fn publish_steps(pins: &GeneratorReleasePins) -> Result<Vec<Yaml>, RenderError> {
    let mut steps = vec![workflow_steps::mise_step(
        pins.setup_for(ReleaseTarget::LinuxX86_64),
    )?];
    steps.push(workflow_steps::install_gh_step(&pins.install_gh_argv)?);
    for asset in assets::ASSETS {
        let artifact_id = format!(
            "${{{{ inputs.{}_artifact_id }}}}",
            artifact_input(asset.target)
        );
        steps.extend(assets::download_build_steps_for_id(
            asset,
            "Download exact release build artifact",
            &artifact_id,
        ));
    }
    steps.push(workflow_steps::download_step_by_id(
        "Download canonical same-run release manifest",
        "${{ inputs.manifest_artifact_id }}",
        manifest::DIR,
    ));
    steps.push(workflow_steps::bash_step_with_env(
        "Verify canonical manifest digest",
        &manifest::manifest_digest_check_script(),
        vec![(
            "VELNOR_RELEASE_MANIFEST_SHA256",
            "${{ inputs.manifest_sha256 }}",
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
        &pins.gh_argv,
    )?);
    steps.push(workflow_steps::publish_step(
        &manifest::publish_script(pins),
        &pins.gh_argv,
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

pub(super) fn local_action_with_input(
    logical: &str,
    name: &str,
    steps: Vec<Yaml>,
    input_name: &str,
    input_description: &str,
    input_value: &str,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<Yaml, RenderError> {
    local_action_with_inputs(
        logical,
        name,
        steps,
        vec![(input_name, input_description, input_value)],
        actions,
    )
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

pub(super) fn artifact_input(target: ReleaseTarget) -> &'static str {
    match target {
        ReleaseTarget::LinuxX86_64 => "linux",
        ReleaseTarget::MacosArm64 => "macos_arm64",
        ReleaseTarget::MacosX86_64 => "macos_x86_64",
    }
}
