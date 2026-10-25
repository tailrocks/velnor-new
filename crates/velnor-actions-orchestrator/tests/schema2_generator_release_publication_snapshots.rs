use super::action_snapshots::{Actions, action};
use super::cross_snapshots;
use super::qualification_snapshots;

pub(super) fn assert_pinned_gh_policy(
    generator: &str,
    actions: &Actions,
) -> Result<(), Box<dyn std::error::Error>> {
    let pinned_wrapper = r#"gh() {\n  (\n    mise --no-config --no-env --no-hooks exec gh@2.103.2 -- gh \"$@\" & _velnor_gh_pid=$!\n    ( sleep 60; kill -TERM \"$_velnor_gh_pid\" 2>/dev/null; sleep 5; kill -KILL \"$_velnor_gh_pid\" 2>/dev/null ) </dev/null >/dev/null 2>&1 & _velnor_gh_watch=$!\n    _velnor_gh_status=0\n    wait \"$_velnor_gh_pid\" || _velnor_gh_status=$?\n    kill -KILL \"$_velnor_gh_watch\" 2>/dev/null || true\n    wait \"$_velnor_gh_watch\" 2>/dev/null || true\n    exit \"$_velnor_gh_status\"\n  ) </dev/null\n}\nexport -f gh"#;
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
    let authority_wrapper = r#"mise --no-config --no-env --no-hooks exec 'gh@'\"$VELNOR_RELEASE_GH_VERSION\" -- gh \"$@\""#;
    for id in [
        "attest-linux",
        "attest-macos",
        "attest-macos-intel",
        "attest-manifest",
        "publish-generator",
    ] {
        let job = super::super::job_body(generator, id)?;
        assert!(
            job.contains(authority_wrapper),
            "{id} must execute GH using validated candidate authority: {job}"
        );
        assert!(
            job.contains("Validate source-bound GitHub CLI version")
                && job.contains("Install source-bound GitHub CLI"),
            "{id} must validate and install its source-bound GH version: {job}"
        );
    }
    Ok(())
}

pub(super) fn assert_asset_catalog(body: &str) {
    let version = env!("CARGO_PKG_VERSION");
    assert!(body.contains("release-manifest.json"), "{body}");
    assert!(
        body.contains("cmp release-manifest.json manifest-assets/release-manifest.json"),
        "{body}"
    );
    assert!(body.contains("sha256sum --check"), "{body}");
    for binary in [
        format!("velnor-actions-{version}-x86_64-unknown-linux-gnu"),
        format!("velnor-actions-{version}-aarch64-apple-darwin"),
        format!("velnor-actions-{version}-x86_64-apple-darwin"),
    ] {
        assert!(body.contains(&binary), "missing {binary}");
    }
    for provenance in [
        format!("velnor-actions-{version}-x86_64-unknown-linux-gnu.provenance.json"),
        format!("velnor-actions-{version}-aarch64-apple-darwin.provenance.json"),
        format!("velnor-actions-{version}-x86_64-apple-darwin.provenance.json"),
    ] {
        assert!(
            body.contains(&provenance),
            "missing provenance {provenance}"
        );
    }
    assert!(!body.contains("ubuntu-26.04-scale-set"), "{body}");
    assert!(!body.contains("runs-on: [velnor"), "{body}");
}

pub(super) fn assert_target_builds(
    body: &str,
    actions: &Actions,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_target_attesters(body)?;
    assert_candidate_qualifications(body, actions)?;
    assert_build_actions(body, actions)
}

fn assert_target_attesters(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    for (attester_id, build_id, qualify_id, runner) in [
        (
            "attest-linux",
            "build-linux",
            "qualify-linux",
            "ubuntu-26.04",
        ),
        ("attest-macos", "build-macos", "qualify-macos", "macos-15"),
        (
            "attest-macos-intel",
            "build-macos-intel",
            "qualify-macos-intel",
            "macos-15-intel",
        ),
    ] {
        let attester = super::super::job_body(body, attester_id)?;
        assert!(
            attester.contains(&format!("runs-on: {runner}\n")),
            "{attester}"
        );
        assert!(attester.contains("id-token: write"), "{attester}");
        assert!(!attester.contains("contents: write"), "{attester}");
        assert!(attester.contains(&format!("- {build_id}\n")), "{attester}");
        assert!(
            attester.contains(&format!("- {qualify_id}\n")),
            "{attester}"
        );
        assert!(attester.contains("- candidate-manifest\n"), "{attester}");
        let download = attester
            .find("name: Download built asset files")
            .ok_or("missing raw build artifact download")?;
        let validate = attester
            .find("name: Validate exact downloaded candidate files")
            .ok_or("missing raw artifact validator")?;
        let provenance = attester
            .find("name: Verify candidate provenance record")
            .ok_or("missing source-bound provenance verification")?;
        let checksum = attester
            .find("name: Verify downloaded checksum sidecar")
            .ok_or("missing sidecar checksum verification")?;
        let attest = attester
            .find("name: Attest built artifacts")
            .ok_or("missing built artifact attestation")?;
        let bundle = attester
            .find("name: Fetch and verify candidate attestation bundles")
            .ok_or("missing candidate attestation bundle verification")?;
        let upload = attester
            .find("name: Upload verified candidate attestation bundles")
            .ok_or("missing candidate attestation bundle upload")?;
        assert!(
            download < validate
                && validate < provenance
                && provenance < checksum
                && checksum < attest
                && attest < bundle
                && bundle < upload,
            "{attester}"
        );
        assert!(
            attester.contains(&format!(
                "artifact-ids: ${{{{ needs.{build_id}.outputs.artifact_id }}}}"
            )),
            "{attester}"
        );
        assert!(
            attester.contains("actions/download-artifact@"),
            "{attester}"
        );
    }
    Ok(())
}

fn assert_candidate_qualifications(
    body: &str,
    actions: &Actions,
) -> Result<(), Box<dyn std::error::Error>> {
    let version = env!("CARGO_PKG_VERSION");
    for (job, action_id, build, directory, binary) in [
        (
            "qualify-linux",
            "generator-release-qualify-linux",
            "build-linux",
            "linux-assets",
            format!("velnor-actions-{version}-x86_64-unknown-linux-gnu"),
        ),
        (
            "qualify-macos",
            "generator-release-qualify-macos",
            "build-macos",
            "macos-assets",
            format!("velnor-actions-{version}-aarch64-apple-darwin"),
        ),
        (
            "qualify-macos-intel",
            "generator-release-qualify-macos-intel",
            "build-macos-intel",
            "macos-intel-assets",
            format!("velnor-actions-{version}-x86_64-apple-darwin"),
        ),
    ] {
        qualification_snapshots::assert_candidate_qualification(
            body, actions, job, action_id, build, directory, &binary,
        )?;
    }
    Ok(())
}

fn assert_build_actions(body: &str, actions: &Actions) -> Result<(), Box<dyn std::error::Error>> {
    for (job_id, action_id) in [
        ("build-linux", "generator-release-build-linux"),
        ("build-macos", "generator-release-build-macos"),
        ("build-macos-intel", "generator-release-build-macos-intel"),
    ] {
        assert_job_action(body, job_id, action_id)?;
    }
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
        linux_action.contains("rust@1.99.1 mr-boxington@1.27.0")
            || linux_action.contains(
                r#"'rust@'\"${VELNOR_TOOL_RUST}\" 'mr-boxington@'\"${VELNOR_TOOL_MR_BOXINGTON}\""#
            ),
        "{linux_action}"
    );
    assert_macos_build(body, actions)
}

fn assert_macos_build(body: &str, actions: &Actions) -> Result<(), Box<dyn std::error::Error>> {
    let macos = super::super::job_body(body, "build-macos")?;
    assert!(macos.contains("runs-on: macos-15\n"), "{macos}");
    let macos_action = action(actions, "generator-release-build-macos")?;
    assert!(macos_action.contains("*Mach-O*arm64*"), "{macos_action}");
    assert!(macos_action.contains("shasum -a 256"), "{macos_action}");
    cross_snapshots::assert_intel_cross_build(body, actions)
}

pub(super) fn assert_publish_job(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    let publish = super::super::job_body(body, "publish-generator")?;
    assert!(publish.contains("runs-on: ubuntu-26.04\n"), "{publish}");
    assert!(publish.contains("contents: write"), "{publish}");
    assert!(!publish.contains("id-token:"), "{publish}");
    let publish_action = publish;
    assert!(
        publish_action.contains("GH_TOKEN: ${{ github.token }}"),
        "{publish_action}"
    );
    assert_publish_bundle_artifacts(publish_action);
    assert_publish_source_and_artifact_bindings(publish_action);
    for attester in ["attest-linux", "attest-macos", "attest-macos-intel"] {
        assert!(publish.contains(&format!("- {attester}\n")), "{publish}");
    }
    assert!(publish.contains("- attest-manifest"), "{publish}");
    assert!(
        publish.contains("environment:\n      name: generator-release"),
        "{publish}"
    );
    Ok(())
}

fn assert_publish_bundle_artifacts(publish_action: &str) {
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
    assert!(
        publish_action.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))),
        "{publish_action}"
    );
}

fn assert_publish_source_and_artifact_bindings(publish_action: &str) {
    for build in ["build-linux", "build-macos", "build-macos-intel"] {
        assert!(
            publish_action.contains(&format!(
                "artifact-ids: ${{{{ needs.{build}.outputs.artifact_id }}}}"
            )),
            "{publish_action}"
        );
    }
    assert!(
        publish_action
            .contains("artifact-ids: ${{ needs.candidate-manifest.outputs.artifact_id }}"),
        "{publish_action}"
    );
    assert!(
        publish_action.contains("${{ needs.candidate-manifest.outputs.manifest_sha256 }}"),
        "{publish_action}"
    );
    assert!(
        !publish_action.contains("artifact-ids: ${{ inputs."),
        "{publish_action}"
    );
    assert!(
        publish_action.contains("actions/download-artifact@"),
        "{publish_action}"
    );
    assert!(
        publish_action.contains("Fetch and verify signed release attestations"),
        "{publish_action}"
    );
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
