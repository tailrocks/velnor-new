use std::collections::BTreeSet;
use std::error::Error;
use std::process::Command;

use velnor_actions_contract::RoutingWorkflow;

use crate::yaml::render_yaml;

use super::{Family, family, render};
use crate::schema2::Schema2WorkflowRequest;

#[path = "schema2_product_release_exec_tests.rs"]
mod exec_tests;

fn workflow(families: &[RoutingWorkflow]) -> Result<String, Box<dyn Error>> {
    let request = Schema2WorkflowRequest {
        version: "2.0.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from_iter(families.iter().copied()),
        mbx_qualification: None,
    };
    let document = render(&request)?.ok_or("release workflow was not rendered")?;
    Ok(render_yaml(&document))
}

#[test]
fn empty_release_request_emits_no_workflow() -> Result<(), Box<dyn Error>> {
    let request = Schema2WorkflowRequest {
        version: "2.0.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::new(),
        mbx_qualification: None,
    };
    assert!(render(&request)?.is_none());
    Ok(())
}

#[test]
fn emits_one_serialized_workflow_with_independent_family_dags() -> Result<(), Box<dyn Error>> {
    let rendered = workflow(&[
        RoutingWorkflow::ImageRelease,
        RoutingWorkflow::MacosBinaryRelease,
        RoutingWorkflow::GeneratorRelease,
    ])?;
    for required in [
        "name: Velnor product releases",
        "cron: 17 * * * *",
        "cancel-in-progress: false",
        "release-eligibility:",
        "prepare-images:",
        "prepare-binary:",
        "prepare-generator:",
        "build-images:",
        "build-binary:",
        "build-linux:",
        "build-macos:",
        "attest-images:",
        "attest-binary:",
        "attest-linux:",
        "attest-macos:",
        "publish-images:",
        "publish-binary:",
        "publish-generator:",
        "needs.release-eligibility.outputs.source_sha",
        "needs.release-eligibility.outputs.workflow_authority_sha",
        "needs.release-eligibility.outputs.ci_run_id",
        "needs.release-eligibility.outputs.ci_attempt",
        "needs.prepare-generator.outputs.action == 'build'",
        "needs.attest-linux.result == 'success'",
        "needs.attest-macos.result == 'success'",
        "signer-workflow",
        "--source-digest",
        "--signer-digest",
    ] {
        assert!(rendered.contains(required), "missing {required}");
    }
    assert!(rendered.contains("branches:\n      - main"));
    assert!(!rendered.contains("image-release.yml"));
    assert!(!rendered.contains("macos-binary-release.yml"));
    assert!(!rendered.contains("generator-release.yml"));
    assert!(
        rendered.contains("ref: ${{ needs.release-eligibility.outputs.source_sha }}"),
        "{rendered}"
    );
    Ok(())
}

#[test]
fn only_requested_product_families_are_composed() -> Result<(), Box<dyn Error>> {
    let rendered = workflow(&[RoutingWorkflow::ImageRelease])?;
    assert!(rendered.contains("release-eligibility:"));
    assert!(rendered.contains("prepare-images:"));
    assert!(rendered.contains("publish-images:"));
    assert!(!rendered.contains("prepare-binary:"));
    assert!(!rendered.contains("prepare-generator:"));
    Ok(())
}

#[test]
fn complete_family_path_skips_artifact_download_and_revalidates() {
    let rendered = family::publish_script(Family::Generator);
    assert!(rendered.contains("if [[ \"$release_action\" == complete ]]"));
    assert!(rendered.contains("already-published release failed idempotent revalidation"));
    assert!(rendered.contains("check_eligibility_identity"));
    assert!(rendered.contains("release verify \"$release_tag\""));
}

#[test]
fn family_scripts_preserve_consumer_paths_assets_and_exact_tag_target() {
    let images = family::prepare_script(Family::Images);
    assert!(images.contains("--dir \"$temp_dir/assets\""));
    assert!(images.contains("velnor-runner-linux-amd64.tar"));
    assert!(images.contains("velnor-dind-linux-amd64.tar"));

    let generator = family::prepare_script(Family::Generator);
    assert!(generator.contains("--dir \"$temp_dir/linux-assets\""));
    assert!(generator.contains("--dir \"$temp_dir/macos-assets\""));
    assert!(
        generator.contains(
            "$temp_dir/linux-assets/velnor-actions-0.1.0-x86_64-unknown-linux-gnu.sha256"
        )
    );
    assert!(
        generator
            .contains("$temp_dir/macos-assets/velnor-actions-0.1.0-aarch64-apple-darwin.sha256")
    );
    assert!(
        generator.contains("tag does not resolve to the exact source commit"),
        "{generator}"
    );

    let publisher = family::publish_script(Family::Binary);
    assert!(publisher.contains("assets/velnor-host"));
    assert!(publisher.contains("velnor-host binary built from $release_source_sha."));
    assert!(!publisher.contains("built from ${VELNOR_SOURCE_SHA}"));
    assert!(publisher.contains("--source-ref refs/heads/main"));
    assert!(!publisher.contains("--clobber"));
    assert!(publisher.contains("--latest=false"));
    assert!(publisher.contains("--draft"));
    assert!(publisher.contains("--verify-tag"));
    let create_ref = publisher.find("git/refs").expect("explicit tag creation");
    let create_release = publisher
        .find("family_gh release create")
        .expect("draft release creation");
    assert!(create_ref < create_release, "{publisher}");
}

#[test]
fn generated_family_shell_scripts_parse_without_execution() -> Result<(), Box<dyn Error>> {
    for family in [Family::Images, Family::Binary, Family::Generator] {
        for script in [
            family::prepare_script(family),
            family::publish_script(family),
        ] {
            let result = Command::new("bash")
                .args(["-n", "-c"])
                .arg(script)
                .output()?;
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    Ok(())
}

#[test]
fn cli_capability_fixture_matches_the_pinned_version_and_required_flags() {
    for capability in [
        "gh version 2.102.0",
        "--latest=false",
        "--draft",
        "--verify-tag",
        "--pattern stringArray",
        "--dir directory",
        "cryptographically signed attestation",
        "--source-digest string",
        "--source-ref string",
        "--signer-workflow string",
        "--signer-digest string",
        "--paginate",
        "--slurp",
    ] {
        assert!(
            family::GH_RELEASE_CAPABILITIES.contains(capability),
            "missing {capability}"
        );
    }
}
