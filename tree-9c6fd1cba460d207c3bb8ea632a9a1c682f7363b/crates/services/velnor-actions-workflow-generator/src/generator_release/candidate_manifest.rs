//! Same-run candidate manifest produced before native qualification.

use velnor_actions_workflow_tree::yaml::Yaml;

use super::GeneratorReleasePins;
use super::{assets, manifest, workflow_steps};
use velnor_actions_contract_release::ReleaseTarget;
use velnor_actions_workflow_tree::job_entries::{base, finish};

/// Build and upload the canonical manifest consumed by qualification and attestation.
pub(super) fn job(
    hosted: Yaml,
    pins: &GeneratorReleasePins,
) -> Result<(String, Yaml), velnor_actions_workflow_steps::RenderError> {
    let candidate_path = manifest::candidate_path();
    let mut steps = Vec::new();
    for product in assets::ASSETS {
        let artifact_id = format!("${{{{ needs.{}.outputs.artifact_id }}}}", product.build_job);
        steps.extend(assets::download_build_steps_for_id(
            product,
            "Download exact build artifact",
            &artifact_id,
        ));
        steps.push(workflow_steps::bash_step(
            "Verify source-bound build record",
            &assets::verify_provenance_script(product, pins),
        ));
    }
    steps.extend([
        workflow_steps::bash_step(
            "Create canonical candidate manifest from verified build bytes",
            &format!(
                "test \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"\nbash scripts/generator-release/create-release-manifest.sh '{}' '{}' '{}' '{}'",
                assets::VERSION,
                assets::REPOSITORY,
                pins.rust_version,
                pins.mr_boxington_version
            ),
        ),
        workflow_steps::bash_step(
            "Stage canonical candidate manifest",
            &format!(
                "set -eu\nmkdir -p {}\nmv {} {}/{}",
                manifest::DIR,
                manifest::FILE,
                manifest::DIR,
                manifest::FILE
            ),
        ),
        workflow_steps::bash_step_with_id(
            "manifest_digest",
            "Record canonical manifest digest",
            &manifest_digest_script(),
        ),
        workflow_steps::upload_step_with_id(
            "upload",
            "Upload same-run release manifest",
            manifest::ARTIFACT,
            &[&candidate_path],
        ),
    ]);
    let needs = [
        "verify-release-source",
        "build-linux",
        "build-macos",
        "build-macos-intel",
    ];
    let mut fields = workflow_steps::with_permissions(
        base("Create candidate release manifest", hosted, 20),
        workflow_steps::perm(&[("actions", "write"), ("contents", "read")]),
    );
    fields.retain(|(key, _)| key != "name");
    fields = workflow_steps::with_needs(fields, &needs);
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![
            (
                "artifact_id".to_owned(),
                Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
            ),
            (
                "manifest_sha256".to_owned(),
                Yaml::str("${{ steps.manifest_digest.outputs.manifest_sha256 }}"),
            ),
        ]),
    ));
    let mut workflow = vec![
        workflow_steps::checkout_step(),
        workflow_steps::mise_step(pins.setup_for(ReleaseTarget::LinuxX86_64))?,
    ];
    workflow.extend(steps);
    Ok(finish("candidate-manifest", fields, workflow))
}

fn manifest_digest_script() -> String {
    format!(
        "set -eu\ndigest=\"$(sha256sum '{path}' | awk 'NR == 1 {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}')\"\ntest \"${{#digest}}\" -eq 64\nprintf 'manifest_sha256=%s\\n' \"$digest\" >> \"$GITHUB_OUTPUT\"",
        path = manifest::candidate_path()
    )
}
