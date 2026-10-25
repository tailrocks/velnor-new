//! Versioned release manifest assembly and immutable publication.

use crate::RenderError;
use crate::yaml::Yaml;
use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::RELEASE_MANIFEST_FILENAME;

use super::super::features::{base, finish};
use super::ProductReleasePins;
use super::assets::{self, ASSETS, REPOSITORY, VERSION};
use super::workflow_steps::{self, with_needs, with_permissions};

/// Published versioned release manifest.
pub(super) const FILE: &str = RELEASE_MANIFEST_FILENAME;
/// Workflow artifact used to carry the attested manifest to publish.
pub(super) const ARTIFACT: &str = "generator-release-manifest";
/// Download location in the publication job.
pub(super) const DIR: &str = "manifest-assets";
/// Same-run canonical manifest path used by candidate qualification and publication.
pub(super) fn candidate_path() -> String {
    format!("{DIR}/{FILE}")
}
/// Download location for detached signed Sigstore bundles.
pub(super) const ATTESTATION_DIR: &str = "release-attestations";

/// Verify product sidecars and build exact versioned asset URLs from their digests.
fn manifest_script(rust_version: &str, mr_boxington_version: &str) -> String {
    format!(
        "bash -s -- '{VERSION}' '{REPOSITORY}' {rust_version} {mr_boxington_version} <<'VELNOR_CREATE_RELEASE_MANIFEST'\n{}\nVELNOR_CREATE_RELEASE_MANIFEST",
        include_str!("../../../scripts/generator-release/create-release-manifest.sh")
    )
}

/// Revalidate and attest the exact same-run candidate manifest.
pub(super) fn job(hosted: Yaml, pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    let mut steps = vec![workflow_steps::mise_step(
        pins.setup_for(velnor_actions_contract::ReleaseTarget::LinuxX86_64),
    )?];
    steps.push(super::jobs::authority_gh_validation_step());
    steps.push(super::jobs::authority_gh_install_step()?);
    steps.extend(product_download_steps());
    steps.extend(candidate_manifest_steps()?);
    let needs = [
        "candidate-manifest",
        "build-linux",
        "build-macos",
        "build-macos-intel",
        "attest-linux",
        "attest-macos",
        "attest-macos-intel",
    ];
    let mut fields = with_needs(
        with_permissions(
            base(
                "Attest generator release manifest",
                hosted,
                JobTimeout::DEFAULT,
            ),
            workflow_steps::perm(&[
                ("actions", "write"),
                ("artifact-metadata", "write"),
                ("attestations", "write"),
                ("contents", "read"),
                ("id-token", "write"),
            ]),
        ),
        &needs,
    );
    fields.retain(|(key, _)| key != "name");
    fields.push((
        "env".to_owned(),
        Yaml::Map(vec![
            (
                "VELNOR_RELEASE_RUST_VERSION".to_owned(),
                Yaml::str("${{ needs.candidate-manifest.outputs.rust_version }}"),
            ),
            (
                "VELNOR_RELEASE_MR_BOXINGTON_VERSION".to_owned(),
                Yaml::str("${{ needs.candidate-manifest.outputs.mr_boxington_version }}"),
            ),
            (
                "VELNOR_RELEASE_GH_VERSION".to_owned(),
                Yaml::str("${{ needs.candidate-manifest.outputs.gh_version }}"),
            ),
        ]),
    ));
    Ok(finish("attest-manifest", fields, steps))
}

fn product_download_steps() -> Vec<Yaml> {
    let mut steps = Vec::new();
    for asset in ASSETS {
        let artifact_id = format!("${{{{ needs.{}.outputs.artifact_id }}}}", asset.build_job);
        steps.extend(assets::download_build_steps_for_id(
            asset,
            &format!("Download {} candidate assets", asset.target.triple()),
            &artifact_id,
        ));
        steps.push(workflow_steps::bash_step_with_env(
            "Verify source-bound candidate provenance",
            &assets::verify_authority_provenance_script(asset),
            vec![
                (
                    "VELNOR_RELEASE_RUST_VERSION",
                    "${{ needs.candidate-manifest.outputs.rust_version }}",
                ),
                (
                    "VELNOR_RELEASE_MR_BOXINGTON_VERSION",
                    "${{ needs.candidate-manifest.outputs.mr_boxington_version }}",
                ),
            ],
        ));
    }
    steps
}

fn candidate_manifest_steps() -> Result<Vec<Yaml>, RenderError> {
    let mut steps = Vec::new();
    steps.push(workflow_steps::download_step_by_id(
        "Download exact canonical candidate manifest",
        "${{ needs.candidate-manifest.outputs.artifact_id }}",
        DIR,
    ));
    steps.push(workflow_steps::bash_step_with_env(
        "Verify canonical candidate manifest digest",
        &manifest_digest_check_script(),
        vec![(
            "VELNOR_RELEASE_MANIFEST_SHA256",
            "${{ needs.candidate-manifest.outputs.manifest_sha256 }}",
        )],
    ));
    let bundle_path = manifest_attestation_bundle_path();
    steps.extend([
        workflow_steps::bash_step(
            "Rebuild and compare the canonical candidate manifest",
            &publication_verify_source_free_script(),
        ),
        workflow_steps::attest_step(&candidate_path()),
        workflow_steps::bash_step_with_token(
            "Fetch and verify manifest attestation bundle",
            &manifest_attestation_bundle_script(),
            &super::jobs::authority_gh_argv(),
        )?,
        workflow_steps::upload_step(
            "Upload verified manifest attestation bundle",
            &format!("{ARTIFACT}-attestations"),
            &[&bundle_path],
        ),
    ]);
    Ok(steps)
}

/// Rebuild the expected manifest from verified downloaded binaries and records.
pub(super) fn publication_verify_script(pins: &ProductReleasePins) -> String {
    let candidate_path = candidate_path();
    format!(
        "{}\ncmp {FILE} {candidate_path}",
        manifest_script(
            &format!("'{}'", pins.rust_version),
            &format!("'{}'", pins.mr_boxington_version),
        )
    )
}

pub(super) fn publication_verify_source_free_script() -> String {
    let candidate_path = candidate_path();
    format!(
        "{}\ncmp {FILE} {candidate_path}",
        manifest_script(
            "\"$VELNOR_RELEASE_RUST_VERSION\"",
            "\"$VELNOR_RELEASE_MR_BOXINGTON_VERSION\"",
        )
    )
}

/// Revalidate the canonical published manifest using the same source bytes.
pub(super) fn verify_published_manifest_script(pins: &ProductReleasePins) -> String {
    publication_verify_script(pins)
}

/// Verify exact canonical manifest bytes downloaded from the candidate job.
pub(super) fn manifest_digest_check_script() -> String {
    let candidate_path = candidate_path();
    format!(
        "set -eu\nexpected=\"$VELNOR_RELEASE_MANIFEST_SHA256\"\nactual=\"$(sha256sum '{candidate_path}' | awk 'NR == 1 {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}')\"\ntest \"${{#expected}}\" -eq 64\ntest \"$actual\" = \"$expected\""
    )
}

/// Assemble release verification and creation in one parent shell.
pub(super) fn publish_script() -> String {
    publish::publish_script()
}

pub(super) fn acceptance_artifact_name() -> String {
    publish::acceptance_artifact_name()
}

pub(super) fn acceptance_artifact_paths() -> [&'static str; 2] {
    publish::acceptance_artifact_paths()
}

pub(super) fn release_asset_path_list() -> Vec<String> {
    publish::release_asset_path_list()
}

#[cfg(test)]
pub(super) fn release_asset_paths() -> String {
    publish::release_asset_paths()
}

#[cfg(test)]
pub(super) fn tag_preflight_script() -> String {
    publish::tag_preflight_script()
}

/// Cryptographically verify every bundle carried through the attest jobs.
pub(super) fn attestation_bundle_script() -> String {
    let candidate_path = candidate_path();
    let mut lines = vec![
        "set -eu".to_owned(),
        "test \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"".to_owned(),
    ];
    for asset in ASSETS {
        for subject in [asset.binary, asset.sidecar, asset.provenance] {
            lines.push(attestation_verify_script(
                &format!("{}/{}", asset.directory, subject),
                subject,
            ));
        }
    }
    lines.push(attestation_verify_script(&candidate_path, FILE));
    lines.join("\n")
}

/// Fetch and verify the three signed bundles after one product attestation.
pub(super) fn asset_attestation_bundle_script(product: assets::ProductAsset) -> String {
    let mut lines = vec![format!("set -eu\nmkdir -p {ATTESTATION_DIR}")];
    for subject in [product.binary, product.sidecar, product.provenance] {
        lines.push(attestation_fetch_script(
            &format!("{}/{}", product.directory, subject),
            subject,
        ));
    }
    lines.join("\n")
}

/// Stable bundle paths uploaded by one product attestation job.
pub(super) fn asset_attestation_bundle_paths(product: assets::ProductAsset) -> Vec<String> {
    [product.binary, product.sidecar, product.provenance]
        .into_iter()
        .map(|name| format!("{ATTESTATION_DIR}/{name}.intoto.jsonl"))
        .collect()
}

/// Fetch and verify the signed manifest bundle after manifest attestation.
pub(super) fn manifest_attestation_bundle_script() -> String {
    let candidate_path = candidate_path();
    format!(
        "set -eu\ntest \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"\nmkdir -p {ATTESTATION_DIR}\n{}",
        attestation_fetch_script(&candidate_path, FILE)
    )
}

/// Stable path uploaded by the manifest attestation job.
pub(super) fn manifest_attestation_bundle_path() -> String {
    format!("{ATTESTATION_DIR}/{FILE}.intoto.jsonl")
}

fn attestation_fetch_script(subject: &str, name: &str) -> String {
    format!(
        "test \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"\nsubject='{subject}'\ndigest=\"$({})\"\ntest \"${{#digest}}\" -eq 64\ncase \"$digest\" in *[!0123456789abcdef]*|'') exit 1 ;; esac\nbundle=\"sha256:${{digest}}.jsonl\"\ntest ! -e \"$bundle\"\ndownloaded=false\nfor attempt in 1 2 3 4 5; do\n  if gh attestation download \"$subject\" --repo \"$GITHUB_REPOSITORY\" --predicate-type https://slsa.dev/provenance/v1 --limit 10 && test -s \"$bundle\"; then downloaded=true; break; fi\n  if test \"$attempt\" -lt 5; then sleep 3; fi\ndone\ntest \"$downloaded\" = true\ngh attestation verify \"$subject\" --repo \"$GITHUB_REPOSITORY\" --bundle \"$bundle\" --source-digest \"$GITHUB_SHA\" --source-ref refs/heads/main --signer-workflow \"${{GITHUB_REPOSITORY}}/.github/workflows/product-release-generator.yml\" --signer-digest \"$GITHUB_SHA\" > /dev/null\nmv \"$bundle\" \"{ATTESTATION_DIR}/{name}.intoto.jsonl\"",
        portable_sha256_command("$subject")
    )
}

fn portable_sha256_command(path: &str) -> String {
    format!(
        "if command -v sha256sum >/dev/null 2>&1; then\n  sha256sum \"{path}\" | awk 'NR == 1 {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}'\nelif command -v shasum >/dev/null 2>&1; then\n  shasum -a 256 \"{path}\" | awk 'NR == 1 {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}'\nelse\n  echo 'no SHA-256 utility is available' >&2\n  exit 1\nfi"
    )
}

fn attestation_verify_script(subject: &str, name: &str) -> String {
    let bundle = format!("{ATTESTATION_DIR}/{name}.intoto.jsonl");
    format!(
        "test -f '{bundle}'\ntest ! -L '{bundle}'\ntest -s '{bundle}'\ngh attestation verify '{subject}' --repo \"$GITHUB_REPOSITORY\" --bundle '{bundle}' --source-digest \"$GITHUB_SHA\" --source-ref refs/heads/main --signer-workflow \"${{GITHUB_REPOSITORY}}/.github/workflows/product-release-generator.yml\" --signer-digest \"$GITHUB_SHA\" > /dev/null"
    )
}

#[cfg(test)]
#[path = "schema2_generator_release_manifest_tests.rs"]
mod tests;

#[path = "schema2_generator_release_manifest_publish.rs"]
mod publish;
