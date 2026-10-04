//! Build, qualify, attest, and publish workflow jobs.

use crate::RenderError;
use crate::composite::{composite_yaml, shared_call_named};
use crate::yaml::Yaml;

use super::super::features::{base, finish};
use super::{assets, manifest, workflow_steps};

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
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let files = [product.binary, product.sidecar, product.provenance];
    let downloads =
        assets::download_build_steps(product, "Download built asset archive", product.build_job);
    let mut action_steps = vec![
        workflow_steps::mise_step(),
        workflow_steps::bash_step(
            "Install pinned GitHub CLI",
            "mise --no-config --no-env --no-hooks install gh@2.102.0",
        ),
    ];
    action_steps.extend(downloads);
    action_steps.extend([
        workflow_steps::bash_step(
            "Verify candidate provenance record",
            &assets::verify_provenance_script(product),
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
        ),
        workflow_steps::upload_step(
            "Upload verified candidate attestation bundles",
            &format!("{}-attestations", product.workflow_artifact),
            &bundle_path_refs,
        ),
    ]);
    let call = local_action(action, name, action_steps, actions)?;
    Ok(finish(
        id,
        workflow_steps::with_needs(
            workflow_steps::with_permissions(
                base(name, runs_on, 20),
                workflow_steps::attest_permissions(),
            ),
            &[product.build_job, product.qualify_job],
        ),
        vec![workflow_steps::checkout_step(), call],
    ))
}

pub(super) fn publish_job(
    hosted: Yaml,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let mut steps = vec![workflow_steps::mise_step()];
    steps.push(workflow_steps::bash_step(
        "Install pinned GitHub CLI",
        "mise --no-config --no-env --no-hooks install gh@2.102.0",
    ));
    steps.extend(
        assets::ASSETS
            .iter()
            .flat_map(|asset| assets::download_steps(*asset, "Download release asset archive")),
    );
    steps.push(workflow_steps::download_step(
        "Download release manifest",
        manifest::ARTIFACT,
        manifest::DIR,
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
    ));
    steps.push(workflow_steps::publish_step(&manifest::publish_script()));
    let call = local_action(
        "generator-release-publish",
        "Publish generator release",
        steps,
        actions,
    )?;
    Ok(finish(
        "publish-generator",
        workflow_steps::with_needs(
            workflow_steps::with_permissions(
                base("Publish velnor-actions", hosted, 30),
                workflow_steps::publish_permissions(),
            ),
            &[
                "attest-linux",
                "attest-macos",
                "attest-macos-intel",
                "attest-manifest",
            ],
        ),
        vec![workflow_steps::checkout_step(), call],
    ))
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
