use std::collections::BTreeSet;
use std::error::Error;
use std::process::Command;

use velnor_actions_contract::{ExecutionMode, RoutingWorkflow};

use crate::schema2::{Schema2WorkflowRequest, render_schema2_workflows};
use crate::workflow_routing::WorkflowRoutingPolicy;
use crate::yaml::render_yaml;

use super::super::product_release_family as family;
use super::{Family, ProductRelease, render};

#[path = "schema2_product_release_exec_tests.rs"]
mod exec_tests;
#[path = "schema2_product_release_prepare_tests.rs"]
mod prepare_tests;
use super::super::product_release_test_pins::test_pins;

fn product(families: &[RoutingWorkflow]) -> Result<ProductRelease, Box<dyn Error>> {
    let request = request(families, ExecutionMode::Hosted)?;
    render(&request)?.ok_or_else(|| "release workflow was not rendered".into())
}

fn request(
    families: &[RoutingWorkflow],
    mode: ExecutionMode,
) -> Result<Schema2WorkflowRequest, crate::RenderError> {
    let scale_set = Schema2WorkflowRequest::canonical_scale_set()?;
    Ok(Schema2WorkflowRequest {
        version: "2.0.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: scale_set.clone(),
        routing: WorkflowRoutingPolicy::from_mode(mode, scale_set),
        workflows: BTreeSet::from_iter(families.iter().copied()),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(test_pins()),
    })
}

#[test]
fn scale_set_mode_routes_image_release_controls_and_writers() -> Result<(), Box<dyn Error>> {
    let request = request(&[RoutingWorkflow::ImageRelease], ExecutionMode::ScaleSet)?;
    let files = render_schema2_workflows(&request)?;
    let rendered = files
        .iter()
        .map(|file| file.bytes.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let scale_runs_on = "runs-on: [velnor, ubuntu-26.04-scale-set]";
    assert!(rendered.contains(scale_runs_on), "{rendered}");
    assert!(!rendered.contains("runs-on: ubuntu-26.04"), "{rendered}");
    Ok(())
}

#[test]
fn scale_set_mode_rejects_macos_binary_release() -> Result<(), Box<dyn Error>> {
    let request = request(
        &[RoutingWorkflow::MacosBinaryRelease],
        ExecutionMode::ScaleSet,
    )?;
    let error =
        render_schema2_workflows(&request).expect_err("macOS jobs cannot use the Linux Scale Set");
    assert!(
        error.to_string().contains("unsupported_darwin_workload:"),
        "{error}"
    );
    Ok(())
}

#[test]
fn scale_set_mode_rejects_generator_release_with_macos_jobs() -> Result<(), Box<dyn Error>> {
    let request = request(
        &[RoutingWorkflow::GeneratorRelease],
        ExecutionMode::ScaleSet,
    )?;
    let error = render_schema2_workflows(&request)
        .expect_err("Scale Set mode must reject macOS release jobs");
    assert!(
        error.to_string().contains("unsupported_darwin_workload:"),
        "{error}"
    );
    Ok(())
}

fn all_text(product: &ProductRelease) -> String {
    let mut documents = vec![render_yaml(&product.workflow)];
    documents.extend(
        product
            .family_workflows
            .iter()
            .map(|(_, workflow)| render_yaml(workflow)),
    );
    documents.extend(
        product
            .actions
            .iter()
            .map(|(_, action)| render_yaml(action)),
    );
    documents.join("\n")
}

#[test]
fn empty_release_request_emits_no_workflow() -> Result<(), Box<dyn Error>> {
    let scale_set = Schema2WorkflowRequest::canonical_scale_set()?;
    let request = Schema2WorkflowRequest {
        version: "2.0.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: scale_set.clone(),
        routing: WorkflowRoutingPolicy::from_mode(ExecutionMode::Hosted, scale_set),
        workflows: BTreeSet::new(),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: None,
    };
    assert!(render(&request)?.is_none());
    Ok(())
}

#[test]
fn all_requested_products_share_one_dispatch_only_source_bound_workflow()
-> Result<(), Box<dyn Error>> {
    let product = product(&[
        RoutingWorkflow::ImageRelease,
        RoutingWorkflow::MacosBinaryRelease,
        RoutingWorkflow::GeneratorRelease,
    ])?;
    let rendered = render_yaml(&product.workflow);
    let all = all_text(&product);
    for required in [
        "name: Velnor product releases",
        "workflow_dispatch: {}",
        "cancel-in-progress: false",
        "release-eligibility:",
        "prepare-images:",
        "prepare-binary:",
        "prepare-generator:",
        "release-images:",
        "release-binary:",
        "release-generator:",
        "needs.release-eligibility.outputs.source_sha",
        "needs.release-eligibility.outputs.workflow_authority_sha",
        "needs.release-eligibility.outputs.ci_run_id",
        "needs.release-eligibility.outputs.ci_attempt",
        "needs.prepare-generator.outputs.action",
    ] {
        assert!(rendered.contains(required), "missing {required}");
    }
    assert_generator_publisher_metadata_surface(&all);
    assert!(!rendered.contains("schedule:"));
    assert!(!rendered.contains("push:"));
    assert!(rendered.contains("workflow_dispatch) ;;"));
    assert!(!rendered.contains("push|schedule|workflow_dispatch"));
    assert!(!all.contains("image-release.yml"));
    assert!(!all.contains("macos-binary-release.yml"));
    assert!(!all.contains("generator-release.yml"));
    assert!(!all.contains("velnor-actions-0.1.0"));
    assert_eq!(product.family_workflows.len(), 3);
    for (path, workflow) in &product.family_workflows {
        assert!(path.starts_with(".github/workflows/product-release-"));
        let body = render_yaml(workflow);
        assert!(body.contains("workflow_call:"), "{path}: {body}");
        assert!(!body.contains("workflow_dispatch:"), "{path}: {body}");
    }
    assert_eq!(
        super::super::generator_release::publication_asset_paths().len(),
        20
    );
    assert!(all.contains("--pattern '"));
    assert!(all.contains("ref: ${{ inputs.source_sha }}"));
    Ok(())
}

fn assert_generator_publisher_metadata_surface(all: &str) {
    for required in [
        "build-images:",
        "build-binary:",
        "build-linux:",
        "build-macos:",
        "build-macos-intel:",
        "candidate-manifest:",
        "qualify-linux:",
        "qualify-macos:",
        "qualify-macos-intel:",
        "attest-images:",
        "attest-binary:",
        "attest-linux:",
        "attest-macos:",
        "attest-macos-intel:",
        "attest-manifest:",
        "publish-images:",
        "publish-binary:",
        "publish-generator:",
        "needs.attest-linux.result == 'success'",
        "needs.attest-macos.result == 'success'",
        "needs.attest-macos-intel.result == 'success'",
        "needs.attest-manifest.result == 'success'",
        "signer-workflow",
        ".github/workflows/product-release-generator.yml",
        "--source-digest",
        "--signer-digest",
        "release-manifest.json",
        "velnor-actions-0.1.13-x86_64-apple-darwin",
        "--target",
        "if $expected_draft then true else .html_url == $html_url end",
    ] {
        assert!(all.contains(required), "missing {required}");
    }
}

#[test]
fn only_requested_families_are_composed() -> Result<(), Box<dyn Error>> {
    let product = product(&[RoutingWorkflow::ImageRelease])?;
    let rendered = render_yaml(&product.workflow);
    assert!(rendered.contains("release-eligibility:"));
    assert!(rendered.contains("prepare-images:"));
    assert!(rendered.contains("release-images:"));
    assert!(!rendered.contains("prepare-binary:"));
    assert!(!rendered.contains("prepare-generator:"));
    assert_eq!(product.family_workflows.len(), 1);
    assert_eq!(
        product.family_workflows[0].0,
        Family::Images.workflow_path()
    );
    Ok(())
}

#[test]
fn generator_publisher_stays_inside_the_typed_release_graph() {
    assert!(family::publish_script(Family::Generator, &test_pins()).is_err());
}

#[test]
fn generator_prepare_uses_the_canonical_inventory_and_manifest_bytes() {
    let generator =
        family::prepare_script(Family::Generator, &test_pins()).expect("generator prepare script");
    for required in [
        "--dir \"$temp_dir/linux-assets\"",
        "--dir \"$temp_dir/macos-assets\"",
        "--dir \"$temp_dir/macos-intel-assets\"",
        "$temp_dir/linux-assets/velnor-actions-0.1.13-x86_64-unknown-linux-gnu.sha256",
        "$temp_dir/macos-assets/velnor-actions-0.1.13-aarch64-apple-darwin.sha256",
        "$temp_dir/macos-intel-assets/velnor-actions-0.1.13-x86_64-apple-darwin.sha256",
        "cmp release-manifest.json manifest-assets/release-manifest.json",
        "readonly fixed_tag='v0.1.13'",
        ".target_commitish == $sha",
        "tag does not resolve to the exact source commit",
    ] {
        assert!(
            generator.contains(required),
            "missing {required}: {generator}"
        );
    }
    assert_eq!(generator.matches("--pattern '").count(), 20);
    assert!(!generator.contains("velnor-actions-0.1.0"));
}

#[test]
fn non_generator_publication_keeps_immutable_tag_and_attestation_checks() {
    let pins = test_pins();
    let images = family::prepare_script(Family::Images, &pins).expect("image prepare script");
    assert!(images.contains("--dir \"$temp_dir/assets\""));
    assert!(images.contains("velnor-runner-linux-amd64.tar"));
    assert!(images.contains("velnor-dind-linux-amd64.tar"));

    let publisher = family::publish_script(Family::Binary, &pins).expect("binary publish script");
    assert!(publisher.contains("assets/velnor-host"));
    assert!(publisher.contains("velnor-host binary built from $release_source_sha."));
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
    for family in [Family::Images, Family::Binary] {
        for script in [
            family::prepare_script(family, &test_pins())?,
            family::publish_script(family, &test_pins())?,
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
    let generator = family::prepare_script(Family::Generator, &test_pins())?;
    let result = Command::new("bash")
        .args(["-n", "-c"])
        .arg(generator)
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
