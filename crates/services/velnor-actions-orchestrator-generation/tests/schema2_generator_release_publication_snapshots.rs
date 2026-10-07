use super::action_snapshots::{Actions, action};
use super::qualification_snapshots;

pub(super) fn assert_pinned_gh_policy(actions: &Actions) {
    let pinned_wrapper = r#"gh() { mise --no-config --no-env --no-hooks exec gh@2.102.0 -- gh \"$@\"; }\nexport -f gh"#;
    for (name, body) in actions {
        if ["gh api ", "gh release ", "gh attestation "]
            .iter()
            .any(|needle| body.contains(needle))
        {
            assert!(
                body.contains(pinned_wrapper),
                "{name} invokes GitHub CLI without the exact Mise pin: {body}"
            );
        }
    }
}

pub(super) fn assert_asset_catalog(body: &str) {
    assert!(body.contains("release-manifest.json"), "{body}");
    assert!(
        body.contains("cmp release-manifest.json manifest-assets/release-manifest.json"),
        "{body}"
    );
    assert!(body.contains("sha256sum --check"), "{body}");
    for binary in [
        "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
        "velnor-actions-0.1.1-aarch64-apple-darwin",
        "velnor-actions-0.1.1-x86_64-apple-darwin",
    ] {
        assert!(body.contains(binary), "missing {binary}");
    }
    for archive in [
        "generator-linux-assets.tar",
        "generator-macos-assets.tar",
        "generator-macos-intel-assets.tar",
    ] {
        assert!(
            body.contains(archive),
            "missing candidate archive {archive}"
        );
    }
    for provenance in [
        "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.provenance.json",
        "velnor-actions-0.1.1-aarch64-apple-darwin.provenance.json",
        "velnor-actions-0.1.1-x86_64-apple-darwin.provenance.json",
    ] {
        assert!(body.contains(provenance), "missing provenance {provenance}");
    }
    assert!(!body.contains("ubuntu-26.04-scale-set"), "{body}");
    assert!(!body.contains("runs-on: [velnor"), "{body}");
    assert!(!body.contains("binary-assets"), "{body}");
    assert!(!body.contains("image-assets"), "{body}");
}

pub(super) fn assert_target_builds(
    body: &str,
    actions: &Actions,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_attest(body, actions, "attest-linux", "ubuntu-26.04")?;
    assert_attest(body, actions, "attest-macos", "macos-15")?;
    assert_attest(body, actions, "attest-macos-intel", "macos-15-intel")?;
    qualification_snapshots::assert_candidate_qualification(
        body,
        actions,
        "qualify-linux",
        "generator-release-qualify-linux",
        "build-linux",
        "linux-assets",
        "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
    )?;
    qualification_snapshots::assert_candidate_qualification(
        body,
        actions,
        "qualify-macos",
        "generator-release-qualify-macos",
        "build-macos",
        "macos-assets",
        "velnor-actions-0.1.1-aarch64-apple-darwin",
    )?;
    qualification_snapshots::assert_candidate_qualification(
        body,
        actions,
        "qualify-macos-intel",
        "generator-release-qualify-macos-intel",
        "build-macos-intel",
        "macos-intel-assets",
        "velnor-actions-0.1.1-x86_64-apple-darwin",
    )?;
    assert_job_action(body, "build-linux", "generator-release-build-linux")?;
    assert_job_action(body, "build-macos", "generator-release-build-macos")?;
    assert_job_action(
        body,
        "build-macos-intel",
        "generator-release-build-macos-intel",
    )?;
    let linux = super::super::job_body(body, "build-linux")?;
    assert!(linux.contains("runs-on: ubuntu-26.04\n"), "{linux}");
    let linux_action = action(actions, "generator-release-build-linux")?;
    assert!(linux_action.contains("ELF"), "{linux_action}");
    assert!(linux_action.contains("sha256sum"), "{linux_action}");
    assert!(
        linux_action.contains(
            "mbx build --release --locked --package velnor-actions-cli --bin velnor-actions"
        ),
        "{linux_action}"
    );
    assert!(
        linux_action.contains("rust@1.98.1 mr-boxington@1.21.1"),
        "{linux_action}"
    );
    let macos = super::super::job_body(body, "build-macos")?;
    assert!(macos.contains("runs-on: macos-15\n"), "{macos}");
    let macos_action = action(actions, "generator-release-build-macos")?;
    assert!(macos_action.contains("*Mach-O*arm64*"), "{macos_action}");
    assert!(macos_action.contains("shasum -a 256"), "{macos_action}");
    let intel = super::super::job_body(body, "build-macos-intel")?;
    assert!(intel.contains("runs-on: macos-15-intel\n"), "{intel}");
    let intel_action = action(actions, "generator-release-build-macos-intel")?;
    assert!(intel_action.contains("*Mach-O*x86_64*"), "{intel_action}");
    assert!(intel_action.contains("shasum -a 256"), "{intel_action}");
    Ok(())
}

pub(super) fn assert_manifest_job(
    body: &str,
    actions: &Actions,
) -> Result<(), Box<dyn std::error::Error>> {
    let candidate = assert_candidate_manifest_job(body)?;
    assert_manifest_attestation_job(body, actions)?;
    assert_manifest_builder_contract(candidate);
    Ok(())
}

fn assert_candidate_manifest_job(body: &str) -> Result<&str, Box<dyn std::error::Error>> {
    let candidate = super::super::job_body(body, "candidate-manifest")?;
    assert!(candidate.contains("runs-on: ubuntu-26.04\n"), "{candidate}");
    assert!(candidate.contains("- verify-release-source"), "{candidate}");
    assert!(candidate.contains("- build-linux"), "{candidate}");
    assert!(candidate.contains("- build-macos"), "{candidate}");
    assert!(candidate.contains("- build-macos-intel"), "{candidate}");
    assert!(candidate.contains("manifest_sha256:"), "{candidate}");
    assert!(
        candidate.contains("Upload same-run release manifest"),
        "{candidate}"
    );
    assert!(
        candidate.contains("manifest-assets/release-manifest.json"),
        "{candidate}"
    );
    Ok(candidate)
}

fn assert_manifest_attestation_job(
    body: &str,
    actions: &Actions,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_job = super::super::job_body(body, "attest-manifest")?;
    assert!(
        manifest_job.contains("runs-on: ubuntu-26.04\n"),
        "{manifest_job}"
    );
    assert!(manifest_job.contains("id-token: write"), "{manifest_job}");
    assert!(!manifest_job.contains("contents: write"), "{manifest_job}");
    assert!(
        manifest_job.contains("- candidate-manifest"),
        "{manifest_job}"
    );
    assert!(manifest_job.contains("- attest-linux"), "{manifest_job}");
    assert!(manifest_job.contains("- attest-macos"), "{manifest_job}");
    assert!(
        manifest_job.contains("- attest-macos-intel"),
        "{manifest_job}"
    );
    assert!(
        manifest_job.contains("uses: ./.github/actions/generator-release-attest-manifest"),
        "{manifest_job}"
    );
    for (name, value) in [
        (
            "linux_artifact_id",
            "${{ needs.build-linux.outputs.artifact_id }}",
        ),
        (
            "macos_arm64_artifact_id",
            "${{ needs.build-macos.outputs.artifact_id }}",
        ),
        (
            "macos_x86_64_artifact_id",
            "${{ needs.build-macos-intel.outputs.artifact_id }}",
        ),
        (
            "manifest_artifact_id",
            "${{ needs.candidate-manifest.outputs.artifact_id }}",
        ),
        (
            "manifest_sha256",
            "${{ needs.candidate-manifest.outputs.manifest_sha256 }}",
        ),
    ] {
        assert!(
            manifest_job.contains(&format!("{name}: {value}")),
            "{manifest_job}"
        );
    }
    let manifest_action = action(actions, "generator-release-attest-manifest")?;
    assert!(
        manifest_action.contains("Verify canonical candidate manifest digest"),
        "{manifest_action}"
    );
    assert!(
        manifest_action.contains("Attest built artifacts"),
        "{manifest_action}"
    );
    assert!(
        manifest_action.contains("Upload verified manifest attestation bundle"),
        "{manifest_action}"
    );
    assert!(
        manifest_action.contains("--signer-digest"),
        "{manifest_action}"
    );
    assert!(!manifest_action.contains("${{ needs."), "{manifest_action}");
    Ok(())
}

fn assert_manifest_builder_contract(manifest: &str) {
    assert!(
        manifest.contains(
            "create-release-manifest.sh '0.1.1' 'tailrocks/velnor-new' '1.98.1' '1.21.1'"
        ),
        "{manifest}"
    );
    assert!(manifest.contains("release-manifest.json"), "{manifest}");
    let manifest_builder =
        include_str!("../../../../scripts/generator-release/create-release-manifest.sh");
    assert!(
        manifest_builder.contains(".schema == 1"),
        "{manifest_builder}"
    );
    assert!(
        manifest_builder.contains(".version == $version"),
        "{manifest_builder}"
    );
    assert!(
        manifest_builder.contains(".commit == $commit"),
        "{manifest_builder}"
    );
    assert!(
        manifest_builder.contains(".toolchain.rust == $rust"),
        "{manifest_builder}"
    );
    assert!(
        manifest_builder.contains(r#".toolchain["mr-boxington"] == $mr_boxington"#),
        "{manifest_builder}"
    );
}

pub(super) fn assert_publish_job(
    body: &str,
    actions: &Actions,
) -> Result<(), Box<dyn std::error::Error>> {
    let publish = super::super::job_body(body, "publish-generator")?;
    assert!(publish.contains("runs-on: ubuntu-26.04\n"), "{publish}");
    assert!(publish.contains("contents: write"), "{publish}");
    assert!(!publish.contains("id-token:"), "{publish}");
    assert_job_action(body, "publish-generator", "generator-release-publish")?;
    let publish_action = action(actions, "generator-release-publish")?;
    assert!(
        publish_action.contains("GH_TOKEN: ${{ github.token }}"),
        "{publish_action}"
    );
    assert_eq!(
        publish_action
            .matches("name: Download verified release attestation bundles")
            .count(),
        4,
        "{publish_action}"
    );
    for artifact in [
        "generator-linux-assets-attestations",
        "generator-macos-assets-attestations",
        "generator-macos-intel-assets-attestations",
        "generator-release-manifest-attestations",
    ] {
        assert!(
            publish_action.contains(&format!("name: {artifact}")),
            "missing downloaded bundle artifact {artifact}: {publish_action}"
        );
    }
    assert!(
        publish_action.contains("gh attestation verify"),
        "{publish_action}"
    );
    assert!(
        publish_action.contains("--source-digest"),
        "{publish_action}"
    );
    assert!(
        publish_action.contains("--source-ref refs/heads/main"),
        "{publish_action}"
    );
    assert!(publish_action.contains(".intoto.jsonl"), "{publish_action}");
    assert!(publish_action.contains("v0.1.1"), "{publish_action}");
    assert!(
        publish_action.contains("inputs:\n  linux_artifact_id:"),
        "{publish_action}"
    );
    assert!(
        publish_action.contains("  manifest_artifact_id:")
            && publish_action.contains("${{ inputs.manifest_sha256 }}"),
        "{publish_action}"
    );
    assert!(!publish_action.contains("${{ needs."), "{publish_action}");
    assert!(publish.contains("- attest-linux"), "{publish}");
    assert!(publish.contains("- attest-macos"), "{publish}");
    assert!(publish.contains("- attest-macos-intel"), "{publish}");
    assert!(publish.contains("- attest-manifest"), "{publish}");
    assert!(
        publish.contains("environment:\n      name: generator-release"),
        "{publish}"
    );
    assert!(
        body.contains("group: generator-release-${{ github.repository }}-${{ github.ref }}"),
        "{body}"
    );
    Ok(())
}

fn assert_job_action(
    workflow: &str,
    id: &str,
    action_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let job = super::super::job_body(workflow, id)?;
    assert!(
        job.contains(&format!("uses: ./.github/actions/{action_name}")),
        "{job}"
    );
    Ok(())
}

fn assert_attest(
    body: &str,
    actions: &Actions,
    id: &str,
    runs_on: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let job = super::super::job_body(body, id)?;
    assert!(job.contains(&format!("runs-on: {runs_on}\n")), "{job}");
    assert!(job.contains("id-token: write"), "{job}");
    assert!(!job.contains("contents: write"), "{job}");
    let action_name = format!("generator-release-{id}");
    assert_job_action(body, id, &action_name)?;
    assert!(action(actions, &action_name)?.contains("provenance.json"));
    Ok(())
}
