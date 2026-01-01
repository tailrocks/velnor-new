//! Versioned release manifest assembly and immutable publication.

use crate::RenderError;
use crate::yaml::Yaml;
use velnor_actions_contract::RELEASE_MANIFEST_FILENAME;

use super::super::features::{base, finish};
use super::GeneratorReleasePins;
use super::assets::{self, ASSETS, REPOSITORY, VERSION};
use super::jobs;
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
        "bash scripts/generator-release/create-release-manifest.sh '{VERSION}' '{REPOSITORY}' '{rust_version}' '{mr_boxington_version}'"
    )
}

/// Revalidate and attest the exact same-run candidate manifest.
pub(super) fn job(
    hosted: Yaml,
    pins: &GeneratorReleasePins,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let mut steps = vec![workflow_steps::mise_step(
        pins.setup_for(velnor_actions_contract::ReleaseTarget::LinuxX86_64),
    )?];
    steps.push(workflow_steps::install_gh_step(&pins.install_gh_argv)?);
    steps.extend(product_download_steps(pins));
    steps.extend(candidate_manifest_steps(pins)?);
    let call = jobs::local_action_with_inputs(
        "generator-release-attest-manifest",
        "Attest generator release manifest",
        steps,
        manifest_action_inputs(),
        actions,
    )?;
    let needs = [
        "candidate-manifest",
        "build-linux",
        "build-macos",
        "attest-linux",
        "attest-macos",
    ];
    let mut fields = with_needs(
        with_permissions(
            base("Attest generator release manifest", hosted, 30),
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
    Ok(finish(
        "attest-manifest",
        fields,
        vec![workflow_steps::checkout_step(), call],
    ))
}

fn product_download_steps(pins: &GeneratorReleasePins) -> Vec<Yaml> {
    let mut steps = Vec::new();
    for asset in ASSETS {
        let artifact_id = format!(
            "${{{{ inputs.{}_artifact_id }}}}",
            jobs::artifact_input(asset.target)
        );
        steps.extend(assets::download_build_steps_for_id(
            asset,
            &format!("Download {} candidate assets", asset.target.triple()),
            &artifact_id,
        ));
        steps.push(workflow_steps::bash_step(
            "Verify source-bound candidate provenance",
            &assets::verify_provenance_script(asset, pins),
        ));
    }
    steps
}

fn candidate_manifest_steps(pins: &GeneratorReleasePins) -> Result<Vec<Yaml>, RenderError> {
    let mut steps = Vec::new();
    steps.push(workflow_steps::download_step_by_id(
        "Download exact canonical candidate manifest",
        "${{ inputs.manifest_artifact_id }}",
        DIR,
    ));
    steps.push(workflow_steps::bash_step_with_env(
        "Verify canonical candidate manifest digest",
        &manifest_digest_check_script(),
        vec![(
            "VELNOR_RELEASE_MANIFEST_SHA256",
            "${{ inputs.manifest_sha256 }}",
        )],
    ));
    let bundle_path = manifest_attestation_bundle_path();
    steps.extend([
        workflow_steps::bash_step(
            "Rebuild and compare the canonical candidate manifest",
            &publication_verify_script(pins),
        ),
        workflow_steps::attest_step(&candidate_path()),
        workflow_steps::bash_step_with_token(
            "Fetch and verify manifest attestation bundle",
            &manifest_attestation_bundle_script(),
            &pins.gh_argv,
        )?,
        workflow_steps::upload_step(
            "Upload verified manifest attestation bundle",
            &format!("{ARTIFACT}-attestations"),
            &[&bundle_path],
        ),
    ]);
    Ok(steps)
}

fn manifest_action_inputs() -> Vec<(&'static str, &'static str, &'static str)> {
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

/// Rebuild the expected manifest from verified downloaded binaries and records.
pub(super) fn publication_verify_script(pins: &GeneratorReleasePins) -> String {
    let candidate_path = candidate_path();
    format!(
        "{}\ncmp {FILE} {candidate_path}",
        manifest_script(&pins.rust_version, &pins.mr_boxington_version)
    )
}

/// Verify exact canonical manifest bytes downloaded from the candidate job.
pub(super) fn manifest_digest_check_script() -> String {
    let candidate_path = candidate_path();
    format!(
        "set -eu\nexpected=\"$VELNOR_RELEASE_MANIFEST_SHA256\"\nactual=\"$(sha256sum '{candidate_path}' | awk 'NR == 1 {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}')\"\ntest \"${{#expected}}\" -eq 64\ntest \"$actual\" = \"$expected\""
    )
}

/// Assemble release verification and creation in one parent shell.
pub(super) fn publish_script(pins: &GeneratorReleasePins) -> String {
    let verify = publication_verify_script(pins);
    let assets = release_asset_paths();
    let preflight = tag_preflight_script();
    let postflight = published_release_verify_script();
    let ci_recheck = assets::ci_check_script();
    format!(
        "set -eu\ntag=\"v{VERSION}\"\n{verify}\n{preflight}\n{ci_recheck}\ngh release create \"$tag\" -R \"${{GITHUB_REPOSITORY}}\" --target \"$GITHUB_SHA\" --title \"velnor-actions $tag\" --latest=false --notes \"velnor-actions {VERSION} built from ${{GITHUB_SHA}}.\" {assets}\n{postflight}"
    )
}

/// Require confirmed 404 responses for both immutable tag and release lookups.
pub(super) fn tag_preflight_script() -> String {
    format!("bash scripts/generator-release/preflight-release-tag.sh '{VERSION}' '{REPOSITORY}'")
}

/// Verify immutable release metadata, target commit, exact asset set, and digests.
pub(super) fn published_release_verify_script() -> String {
    let paths = release_asset_path_list();
    let bash_paths = paths
        .iter()
        .map(|path| format!("  '{path}'"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "set -eu\ntest \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"\ntag=\"v{VERSION}\"\nref=\"$(gh api \"repos/$GITHUB_REPOSITORY/git/ref/tags/$tag\")\"\nref_type=\"$(printf '%s\\n' \"$ref\" | jq -er .object.type)\"\nref_sha=\"$(printf '%s\\n' \"$ref\" | jq -er .object.sha)\"\ncase \"$ref_type\" in\n  commit) tag_commit=\"$ref_sha\" ;;\n  tag) tag_commit=\"$(gh api \"repos/$GITHUB_REPOSITORY/git/tags/$ref_sha\" --jq .object.sha)\" ;;\n  *) echo \"release tag has unexpected object type: $ref_type\" >&2; exit 1 ;;\nesac\ntest \"$tag_commit\" = \"$GITHUB_SHA\"\nrelease=\"$(gh api \"repos/$GITHUB_REPOSITORY/releases/tags/$tag\")\"\nprintf '%s\\n' \"$release\" | jq -e --arg tag \"$tag\" '.tag_name == $tag and .draft == false and .prerelease == false and .immutable == true' > /dev/null\npaths=(\n{bash_paths}\n)\ntest \"$(printf '%s\\n' \"$release\" | jq -r '.assets | length')\" -eq {}\nfor path in \"${{paths[@]}}\"; do\n  name=\"${{path##*/}}\"\n  digest=\"$(sha256sum \"$path\" | awk 'NR == 1 {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}')\"\n  expected=\"sha256:$digest\"\n  actual=\"$(printf '%s\\n' \"$release\" | jq -er --arg name \"$name\" '[.assets[] | select(.name == $name)] | if length == 1 then .[0].digest else error(\"missing or duplicate release asset\") end')\"\n  test \"$actual\" = \"$expected\"\ndone",
        paths.len()
    )
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
        "test \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"\nsubject='{subject}'\ndigest=\"$({})\"\ntest \"${{#digest}}\" -eq 64\ncase \"$digest\" in *[!0123456789abcdef]*|'') exit 1 ;; esac\nbundle=\"sha256:${{digest}}.jsonl\"\ntest ! -e \"$bundle\"\ndownloaded=false\nfor attempt in 1 2 3 4 5; do\n  if gh attestation download \"$subject\" --repo \"$GITHUB_REPOSITORY\" --predicate-type https://slsa.dev/provenance/v1 --limit 10 && test -s \"$bundle\"; then downloaded=true; break; fi\n  if test \"$attempt\" -lt 5; then sleep 3; fi\ndone\ntest \"$downloaded\" = true\ngh attestation verify \"$subject\" --repo \"$GITHUB_REPOSITORY\" --bundle \"$bundle\" --source-digest \"$GITHUB_SHA\" --source-ref refs/heads/main --signer-workflow \"${{GITHUB_REPOSITORY}}/.github/workflows/generator-release.yml\" --signer-digest \"$GITHUB_SHA\" > /dev/null\nmv \"$bundle\" \"{ATTESTATION_DIR}/{name}.intoto.jsonl\"",
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
        "test -f '{bundle}'\ntest ! -L '{bundle}'\ntest -s '{bundle}'\ngh attestation verify '{subject}' --repo \"$GITHUB_REPOSITORY\" --bundle '{bundle}' --source-digest \"$GITHUB_SHA\" --source-ref refs/heads/main --signer-workflow \"${{GITHUB_REPOSITORY}}/.github/workflows/generator-release.yml\" --signer-digest \"$GITHUB_SHA\" > /dev/null"
    )
}

/// Release assets published in stable target order, followed by the manifest.
pub(super) fn release_asset_paths() -> String {
    release_asset_path_list().join(" ")
}

fn release_asset_path_list() -> Vec<String> {
    let mut paths = ASSETS
        .iter()
        .flat_map(|asset| {
            [
                format!("{}/{}", asset.directory, asset.binary),
                format!("{}/{}", asset.directory, asset.sidecar),
                format!("{}/{}", asset.directory, asset.provenance),
            ]
        })
        .collect::<Vec<_>>();
    paths.push(format!("{DIR}/{FILE}"));
    for asset in ASSETS {
        for name in [asset.binary, asset.sidecar, asset.provenance] {
            paths.push(format!("{ATTESTATION_DIR}/{name}.intoto.jsonl"));
        }
    }
    paths.push(format!("{ATTESTATION_DIR}/{FILE}.intoto.jsonl"));
    paths
}

#[cfg(test)]
#[path = "schema2_generator_release_manifest_tests.rs"]
mod tests;
