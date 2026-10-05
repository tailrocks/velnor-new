use std::collections::BTreeSet;

use velnor_actions_contract::RoutingWorkflow;

use super::generator_release;
use crate::schema2::Schema2WorkflowRequest;

#[test]
fn release_workflow_binds_v011_assets_and_single_writer() {
    let request = Schema2WorkflowRequest {
        version: "0.1.1".to_owned(),
        hosted_label: "ubuntu-22.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set().expect("valid scale set"),
        workflows: BTreeSet::from([RoutingWorkflow::GeneratorRelease]),
        mbx_qualification: None,
    };
    let yaml = generator_release(&request).expect("valid v0.1.1 workflow");
    let workflow = crate::yaml::render_yaml(&yaml);

    for expected in [
        "runs-on: ubuntu-22.04",
        "runs-on: macos-15",
        "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
        "velnor-actions-0.1.1-aarch64-apple-darwin",
        "velnor-actions-release-manifest.json",
        "velnor-actions-0.1.1-linux-provenance.sigstore.json",
        "velnor-actions-0.1.1-macos-provenance.sigstore.json",
        "velnor-actions-0.1.1-manifest-provenance.sigstore.json",
        "mr-boxington@1.21.1",
        "gh@2.102.0",
        "persist-credentials: \"false\"",
        "overwrite: \"false\"",
    ] {
        assert!(
            workflow.contains(expected),
            "missing {expected}: {workflow}"
        );
    }
    assert_eq!(workflow.matches("contents: write").count(), 1, "{workflow}");
    assert_eq!(workflow.matches("id-token: write").count(), 3, "{workflow}");
    assert_eq!(
        workflow.matches("actions/upload-artifact@").count(),
        6,
        "{workflow}"
    );
    assert_eq!(
        workflow.matches("actions/attest-build-provenance@").count(),
        3,
        "{workflow}"
    );

    let publish = job(&workflow, "publish-generator");
    assert!(publish.contains("actions: read"), "{publish}");
    assert!(publish.contains("contents: write"), "{publish}");
    assert!(publish.contains("digest-mismatch: error"), "{publish}");
    assert!(
        publish.contains("bash scripts/generator-release/publish.sh"),
        "{publish}"
    );
    assert!(!publish.contains("cargo build"), "{publish}");
    assert!(!publish.contains("velnor-actions --version"), "{publish}");
}

#[test]
fn release_workflow_rejects_non_numeric_versions() {
    let request = Schema2WorkflowRequest {
        version: "0.1.1-rc.1".to_owned(),
        hosted_label: "ubuntu-22.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set().expect("valid scale set"),
        workflows: BTreeSet::from([RoutingWorkflow::GeneratorRelease]),
        mbx_qualification: None,
    };
    assert!(generator_release(&request).is_err());
}

fn job(workflow: &str, name: &str) -> String {
    let marker = format!("  {name}:\n");
    let start = workflow.find(&marker).expect("job header");
    let rest = &workflow[start + marker.len()..];
    let end = rest.find("  \n").unwrap_or(rest.len());
    rest[..end].to_owned()
}
