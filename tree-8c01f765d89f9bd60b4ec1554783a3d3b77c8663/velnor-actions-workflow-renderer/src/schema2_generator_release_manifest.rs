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
    let mut steps = ASSETS
        .iter()
        .flat_map(|asset| {
            assets::download_steps(*asset, &format!("Download {} assets", asset.target))
        })
        .collect::<Vec<_>>();
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
    format!(
        "set -eu\ntag=\"v{VERSION}\"\n{verify}\n{preflight}\ngh release create \"$tag\" -R \"${{GITHUB_REPOSITORY}}\" --target \"$GITHUB_SHA\" --title \"velnor-actions $tag\" --latest=false --notes \"velnor-actions {VERSION} built from ${{GITHUB_SHA}}.\" {assets}\n{postflight}"
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
        "set -eu\ntag=\"v{VERSION}\"\nref=\"$(gh api \"repos/$GITHUB_REPOSITORY/git/ref/tags/$tag\")\"\nref_type=\"$(printf '%s\\n' \"$ref\" | jq -er .object.type)\"\nref_sha=\"$(printf '%s\\n' \"$ref\" | jq -er .object.sha)\"\ncase \"$ref_type\" in\n  commit) tag_commit=\"$ref_sha\" ;;\n  tag) tag_commit=\"$(gh api \"repos/$GITHUB_REPOSITORY/git/tags/$ref_sha\" --jq .object.sha)\" ;;\n  *) echo \"release tag has unexpected object type: $ref_type\" >&2; exit 1 ;;\nesac\ntest \"$tag_commit\" = \"$GITHUB_SHA\"\nrelease=\"$(gh api \"repos/$GITHUB_REPOSITORY/releases/tags/$tag\")\"\nprintf '%s\\n' \"$release\" | jq -e --arg tag \"$tag\" '.tag_name == $tag and .draft == false and .prerelease == false and .immutable == true' > /dev/null\npaths=(\n{bash_paths}\n)\ntest \"$(printf '%s\\n' \"$release\" | jq -r '.assets | length')\" -eq {}\nfor path in \"${{paths[@]}}\"; do\n  name=\"${{path##*/}}\"\n  digest=\"$(sha256sum \"$path\" | awk 'NR == 1 {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}')\"\n  expected=\"sha256:$digest\"\n  actual=\"$(printf '%s\\n' \"$release\" | jq -er --arg name \"$name\" '[.assets[] | select(.name == $name)] | if length == 1 then .[0].digest else error(\"missing or duplicate release asset\") end')\"\n  test \"$actual\" = \"$expected\"\ndone",
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
    format!(
        "subject='{subject}'\ndigest=\"$(sha256sum \"$subject\" | awk 'NR == 1 {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}')\"\ntest \"${{#digest}}\" -eq 64\ncase \"$digest\" in *[!0123456789abcdef]*|'') exit 1 ;; esac\nbundle=\"sha256:${{digest}}.jsonl\"\ntest ! -e \"$bundle\"\ndownloaded=false\nfor attempt in 1 2 3 4 5; do\n  if gh attestation download \"$subject\" --repo \"$GITHUB_REPOSITORY\" --predicate-type https://slsa.dev/provenance/v1 --limit 10 && test -s \"$bundle\"; then downloaded=true; break; fi\n  if test \"$attempt\" -lt 5; then sleep 3; fi\ndone\ntest \"$downloaded\" = true\ngh attestation verify \"$subject\" --repo \"$GITHUB_REPOSITORY\" --bundle \"$bundle\" --source-digest \"$GITHUB_SHA\" --source-ref refs/heads/main --signer-workflow \"${{GITHUB_REPOSITORY}}/.github/workflows/generator-release.yml\" > /dev/null\nmv \"$bundle\" \"{ATTESTATION_DIR}/{name}.intoto.jsonl\""
    )
}

fn attestation_verify_script(subject: &str, name: &str) -> String {
    let bundle = format!("{ATTESTATION_DIR}/{name}.intoto.jsonl");
    format!(
        "test -f '{bundle}'\ntest ! -L '{bundle}'\ntest -s '{bundle}'\ngh attestation verify '{subject}' --repo \"$GITHUB_REPOSITORY\" --bundle '{bundle}' --source-digest \"$GITHUB_SHA\" --source-ref refs/heads/main --signer-workflow \"${{GITHUB_REPOSITORY}}/.github/workflows/generator-release.yml\" > /dev/null"
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
mod tests {
    use super::{FILE, manifest_attestation_bundle_script, manifest_script, tag_preflight_script};
    use std::error::Error;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    fn scratch() -> Result<Scratch, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "velnor-release-preflight-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&directory)?;
        Ok(Scratch(directory))
    }

    #[test]
    fn manifest_bundle_uses_the_created_and_attested_file() {
        let create = manifest_script();
        let fetch = manifest_attestation_bundle_script();
        assert!(create.contains("create-release-manifest.sh"));
        assert!(fetch.contains(&format!("subject='{FILE}'")), "{fetch}");
        assert!(!fetch.contains("manifest-assets/release-manifest.json"));
    }

    #[test]
    fn tag_preflight_accepts_only_confirmed_not_found_responses() -> Result<(), Box<dyn Error>> {
        for (case, accepted) in [
            ("confirmed-404", true),
            ("forbidden", false),
            ("network", false),
            ("malformed", false),
            ("missing-status", false),
        ] {
            let scratch = scratch()?;
            let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()?;
            let gh = scratch.0.join("gh");
            fs::write(
                &gh,
                r#"#!/bin/sh
printf '%s\n' "$*" >> "$GH_CALLS"
case "$GH_CASE" in
  confirmed-404) printf 'HTTP/2 404\r\ncontent-type: application/json\r\n\r\n{"message":"Not Found","status":"404"}\n'; exit 1 ;;
  forbidden) printf 'HTTP/2 403\r\n\r\n{"message":"Forbidden","status":"403"}\n'; exit 1 ;;
  network) exit 22 ;;
  malformed) printf 'not-an-http-response\n\nnot-json\n'; exit 1 ;;
  missing-status) printf 'HTTP/2 404\r\n\r\n{"message":"Not Found"}\n'; exit 1 ;;
esac
"#,
            )?;
            let mut permissions = fs::metadata(&gh)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&gh, permissions)?;
            let mut paths = vec![scratch.0.clone()];
            paths.extend(std::env::split_paths(
                &std::env::var_os("PATH").ok_or("missing PATH")?,
            ));
            let path = std::env::join_paths(paths)?;
            let output = Command::new("bash")
                .arg("-c")
                .arg(tag_preflight_script())
                .current_dir(workspace)
                .env("PATH", path)
                .env("GH_CALLS", scratch.0.join("calls"))
                .env("GH_CASE", case)
                .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
                .output()?;
            let calls = fs::read_to_string(scratch.0.join("calls"))?;
            assert_eq!(
                output.status.success(),
                accepted,
                "case {case}; stderr: {}; calls: {calls}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                calls.lines().next(),
                Some("api --include repos/tailrocks/velnor-new/git/ref/tags/v0.1.1")
            );
            assert_eq!(calls.lines().count(), if accepted { 2 } else { 1 });
            if accepted {
                assert_eq!(
                    calls.lines().nth(1),
                    Some("api --include repos/tailrocks/velnor-new/releases/tags/v0.1.1")
                );
            }
        }
        Ok(())
    }
}
