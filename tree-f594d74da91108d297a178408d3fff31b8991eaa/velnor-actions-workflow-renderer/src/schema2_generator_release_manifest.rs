//! Versioned release manifest assembly and immutable publication.

use crate::RenderError;
use crate::yaml::Yaml;

use super::super::features::{base, finish};
use super::assets::{self, ASSETS, REPOSITORY, VERSION};
use super::workflow_steps::{self, upload_step, with_needs, with_permissions};

/// Published versioned release manifest.
pub(super) const FILE: &str = "release-manifest.json";
/// Workflow artifact used to carry the attested manifest to publish.
pub(super) const ARTIFACT: &str = "generator-release-manifest";
/// Download location in the publication job.
pub(super) const DIR: &str = "manifest-assets";
/// Download location for detached signed Sigstore bundles.
pub(super) const ATTESTATION_DIR: &str = "release-attestations";

/// Verify product sidecars and build exact versioned asset URLs from their digests.
fn manifest_script() -> String {
    format!("bash scripts/generator-release/create-release-manifest.sh '{VERSION}' '{REPOSITORY}'")
}

/// Attest the manifest after validating all downloaded product records.
pub(super) fn job(
    hosted: Yaml,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let needs = ASSETS
        .iter()
        .map(|asset| asset.attest_job)
        .collect::<Vec<_>>();
    let mut steps = vec![
        workflow_steps::mise_step(),
        workflow_steps::bash_step("Install pinned GitHub CLI", &assets::install_pinned_gh()),
    ];
    steps.extend(ASSETS.iter().flat_map(|asset| {
        assets::download_steps(*asset, &format!("Download {} assets", asset.target))
    }));
    let bundle_path = manifest_attestation_bundle_path();
    steps.extend([
        workflow_steps::bash_step(
            "Verify assets and create release manifest",
            &manifest_script(),
        ),
        workflow_steps::attest_step(FILE),
        workflow_steps::bash_step_with_token(
            "Fetch and verify manifest attestation bundle",
            &manifest_attestation_bundle_script(),
        ),
        workflow_steps::upload_step(
            "Upload verified manifest attestation bundle",
            &format!("{ARTIFACT}-attestations"),
            &[&bundle_path],
        ),
        upload_step("Upload release manifest", ARTIFACT, &[FILE]),
    ]);
    let call = super::jobs::local_action(
        "generator-release-manifest",
        "Attest generator release manifest",
        steps,
        actions,
    )?;
    Ok(finish(
        "attest-manifest",
        with_needs(
            with_permissions(
                base("Attest generator release manifest", hosted, 20),
                workflow_steps::perm(&[
                    ("actions", "write"),
                    ("artifact-metadata", "write"),
                    ("attestations", "write"),
                    ("contents", "read"),
                    ("id-token", "write"),
                ]),
            ),
            &needs,
        ),
        vec![workflow_steps::checkout_step(), call],
    ))
}

/// Rebuild the expected manifest from verified downloaded binaries and records.
pub(super) fn publication_verify_script() -> String {
    format!("{}\ncmp {FILE} {DIR}/{FILE}", manifest_script())
}

/// Assemble release verification and creation in one parent shell.
pub(super) fn publish_script() -> String {
    let verify = publication_verify_script();
    let assets = release_asset_paths();
    let preflight = tag_preflight_script();
    let postflight = published_release_verify_script();
    let create = assets::pinned_gh(&format!(
        "release create \"$tag\" -R \"${{GITHUB_REPOSITORY}}\" --target \"$GITHUB_SHA\" --title \"velnor-actions $tag\" --latest=false --notes \"velnor-actions {VERSION} built from ${{GITHUB_SHA}}.\" {assets}"
    ));
    format!("set -eu\ntag=\"v{VERSION}\"\n{verify}\n{preflight}\n{create}\n{postflight}")
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
    let tag_ref = assets::pinned_gh("api \"repos/$GITHUB_REPOSITORY/git/ref/tags/$tag\"");
    let tag_object =
        assets::pinned_gh("api \"repos/$GITHUB_REPOSITORY/git/tags/$ref_sha\" --jq .object.sha");
    let release_lookup = assets::pinned_gh("api \"repos/$GITHUB_REPOSITORY/releases/tags/$tag\"");
    format!(
        "set -eu\ntag=\"v{VERSION}\"\nref=\"$({tag_ref})\"\nref_type=\"$(printf '%s\\n' \"$ref\" | jq -er .object.type)\"\nref_sha=\"$(printf '%s\\n' \"$ref\" | jq -er .object.sha)\"\ncase \"$ref_type\" in\n  commit) tag_commit=\"$ref_sha\" ;;\n  tag) tag_commit=\"$({tag_object})\" ;;\n  *) echo \"release tag has unexpected object type: $ref_type\" >&2; exit 1 ;;\nesac\ntest \"$tag_commit\" = \"$GITHUB_SHA\"\nrelease=\"$({release_lookup})\"\nprintf '%s\\n' \"$release\" | jq -e --arg tag \"$tag\" '.tag_name == $tag and .draft == false and .prerelease == false and .immutable == true' > /dev/null\npaths=(\n{bash_paths}\n)\ntest \"$(printf '%s\\n' \"$release\" | jq -r '.assets | length')\" -eq {}\nfor path in \"${{paths[@]}}\"; do\n  name=\"${{path##*/}}\"\n  digest=\"$(sha256sum \"$path\" | awk 'NR == 1 {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}')\"\n  expected=\"sha256:$digest\"\n  actual=\"$(printf '%s\\n' \"$release\" | jq -er --arg name \"$name\" '[.assets[] | select(.name == $name)] | if length == 1 then .[0].digest else error(\"missing or duplicate release asset\") end')\"\n  test \"$actual\" = \"$expected\"\ndone",
        paths.len()
    )
}

/// Cryptographically verify every bundle carried through the attest jobs.
pub(super) fn attestation_bundle_script() -> String {
    let mut lines = vec!["set -eu".to_owned()];
    for asset in ASSETS {
        for subject in [asset.binary, asset.sidecar, asset.provenance] {
            lines.push(attestation_verify_script(
                &format!("{}/{}", asset.directory, subject),
                subject,
            ));
        }
    }
    lines.push(attestation_verify_script(&format!("{DIR}/{FILE}"), FILE));
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
    format!(
        "set -eu\nmkdir -p {ATTESTATION_DIR}\n{}",
        attestation_fetch_script(FILE, FILE)
    )
}

/// Stable path uploaded by the manifest attestation job.
pub(super) fn manifest_attestation_bundle_path() -> String {
    format!("{ATTESTATION_DIR}/{FILE}.intoto.jsonl")
}

fn attestation_fetch_script(subject: &str, name: &str) -> String {
    let download = assets::pinned_gh(
        "attestation download \"$subject\" --repo \"$GITHUB_REPOSITORY\" --predicate-type https://slsa.dev/provenance/v1 --limit 10",
    );
    let verify = assets::pinned_gh(
        "attestation verify \"$subject\" --repo \"$GITHUB_REPOSITORY\" --bundle \"$bundle\" --source-digest \"$GITHUB_SHA\" --source-ref refs/heads/main --signer-workflow \"${{GITHUB_REPOSITORY}}/.github/workflows/generator-release.yml\" > /dev/null",
    );
    format!(
        "subject='{subject}'\ndigest=\"$(sha256sum \"$subject\" | awk 'NR == 1 {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}')\"\ntest \"${{#digest}}\" -eq 64\ncase \"$digest\" in *[!0123456789abcdef]*|'') exit 1 ;; esac\nbundle=\"sha256:${{digest}}.jsonl\"\ntest ! -e \"$bundle\"\ndownloaded=false\nfor attempt in 1 2 3 4 5; do\n  if {download} && test -s \"$bundle\"; then downloaded=true; break; fi\n  if test \"$attempt\" -lt 5; then sleep 3; fi\ndone\ntest \"$downloaded\" = true\n{verify}\nmv \"$bundle\" \"{ATTESTATION_DIR}/{name}.intoto.jsonl\""
    )
}

fn attestation_verify_script(subject: &str, name: &str) -> String {
    let bundle = format!("{ATTESTATION_DIR}/{name}.intoto.jsonl");
    let verify = assets::pinned_gh(&format!(
        "attestation verify '{subject}' --repo \"$GITHUB_REPOSITORY\" --bundle '{bundle}' --source-digest \"$GITHUB_SHA\" --source-ref refs/heads/main --signer-workflow \"${{GITHUB_REPOSITORY}}/.github/workflows/generator-release.yml\" > /dev/null"
    ));
    format!("test -f '{bundle}'\ntest ! -L '{bundle}'\ntest -s '{bundle}'\n{verify}")
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
