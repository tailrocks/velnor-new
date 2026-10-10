use std::collections::BTreeSet;
use std::error::Error;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use velnor_actions_contract::RoutingWorkflow;

use crate::schema2::Schema2WorkflowRequest;
use crate::yaml::render_yaml;

use super::super::product_release_family as family;
use super::{Family, ProductRelease, render};

#[path = "schema2_product_release_exec_tests.rs"]
mod exec_tests;
use super::super::product_release_test_pins::test_pins;

fn product(families: &[RoutingWorkflow]) -> Result<ProductRelease, Box<dyn Error>> {
    let request = Schema2WorkflowRequest {
        version: "2.0.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from_iter(families.iter().copied()),
        mbx_qualification: None,
        mise_pin_qualification: None,
        rust_toolchain_qualification: None,
        product_release: Some(test_pins()),
    };
    render(&request)?.ok_or_else(|| "release workflow was not rendered".into())
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
    let request = Schema2WorkflowRequest {
        version: "2.0.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::new(),
        mbx_qualification: None,
        mise_pin_qualification: None,
        rust_toolchain_qualification: None,
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
        "release_family:",
        "type: choice",
        "required: false",
        "default: all",
        "options:",
        "          - all",
        "          - binary",
        "          - generator",
        "          - images",
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
    for family in ["images", "binary", "generator"] {
        let condition =
            format!("inputs.release_family == 'all' || inputs.release_family == '{family}'");
        assert_eq!(rendered.matches(&condition).count(), 2, "{condition}");
    }
    let eligibility = rendered
        .split("  release-eligibility:\n")
        .nth(1)
        .and_then(|jobs| jobs.split("\n  prepare-images:").next())
        .ok_or("release eligibility job is missing")?;
    assert!(
        !eligibility.contains("\n    if:"),
        "shared exact-source eligibility must always run: {eligibility}"
    );
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
        "velnor-actions-0.1.7-x86_64-apple-darwin",
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
    assert!(rendered.contains("          - all\n          - images\n"));
    assert!(!rendered.contains("          - binary\n"));
    assert!(!rendered.contains("          - generator\n"));
    let selected_condition =
        "if: inputs.release_family == 'all' || inputs.release_family == 'images'";
    assert_eq!(rendered.matches(selected_condition).count(), 2);
    assert_eq!(rendered.matches("if: inputs.release_family").count(), 2);
    for unsupported in ["binary", "generator", "malformed"] {
        let condition = format!(
            "if: inputs.release_family == 'all' || inputs.release_family == '{unsupported}'"
        );
        assert!(
            !rendered.contains(&condition),
            "unexpected condition {condition}"
        );
    }
    assert_eq!(product.family_workflows.len(), 1);
    assert_eq!(
        product.family_workflows[0].0,
        Family::Images.workflow_path()
    );
    Ok(())
}

#[path = "schema2_product_release_workflow_tests.rs"]
mod generator_tests;
