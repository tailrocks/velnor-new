use crate::yaml::Yaml;
use velnor_actions_contract::JobTimeout;

use super::super::features::{base, finish};
use super::ProductReleasePins;
use super::{assets, manifest, workflow_steps};
use velnor_actions_contract::ReleaseTarget;

const NEEDS: [&str; 4] = [
    "verify-release-source",
    "build-linux",
    "build-macos",
    "build-macos-intel",
];

pub(super) fn job(
    hosted: Yaml,
    pins: &ProductReleasePins,
) -> Result<(String, Yaml), crate::RenderError> {
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
        workflow_steps::bash_step_with_id(
            "authority_toolchain",
            "Export verified release toolchain authority",
            &format!(
                "set -eu\nrust='{}'\nmbx='{}'\ngh='{}'\nexact_semver='^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$'\n[[ \"$rust\" =~ $exact_semver && \"$mbx\" =~ $exact_semver && \"$gh\" =~ $exact_semver ]] || exit 1\nprintf 'rust_version=%s\\nmr_boxington_version=%s\\ngh_version=%s\\n' \"$rust\" \"$mbx\" \"$gh\" >> \"$GITHUB_OUTPUT\"",
                pins.rust_version, pins.mr_boxington_version, pins.gh_version
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
    let mut fields = workflow_steps::with_permissions(
        base(
            "Create candidate release manifest",
            hosted,
            JobTimeout::DEFAULT,
        ),
        workflow_steps::perm(&[("actions", "write"), ("contents", "read")]),
    );
    fields.retain(|(key, _)| key != "name");
    fields = workflow_steps::with_needs(fields, &NEEDS);
    fields.push(("outputs".to_owned(), candidate_outputs()));
    let mut workflow = vec![
        workflow_steps::checkout_step(),
        workflow_steps::mise_step(pins.setup_for(ReleaseTarget::LinuxX86_64))?,
    ];
    workflow.extend(steps);
    Ok(finish("candidate-manifest", fields, workflow))
}

fn candidate_outputs() -> Yaml {
    Yaml::Map(vec![
        (
            "artifact_id".to_owned(),
            Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
        ),
        (
            "manifest_sha256".to_owned(),
            Yaml::str("${{ steps.manifest_digest.outputs.manifest_sha256 }}"),
        ),
        (
            "rust_version".to_owned(),
            Yaml::str("${{ steps.authority_toolchain.outputs.rust_version }}"),
        ),
        (
            "mr_boxington_version".to_owned(),
            Yaml::str("${{ steps.authority_toolchain.outputs.mr_boxington_version }}"),
        ),
        (
            "gh_version".to_owned(),
            Yaml::str("${{ steps.authority_toolchain.outputs.gh_version }}"),
        ),
    ])
}

fn manifest_digest_script() -> String {
    format!(
        "set -eu\ndigest=\"$(sha256sum '{path}' | awk 'NR == 1 {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}')\"\ntest \"${{#digest}}\" -eq 64\nprintf 'manifest_sha256=%s\\n' \"$digest\" >> \"$GITHUB_OUTPUT\"",
        path = manifest::candidate_path()
    )
}
