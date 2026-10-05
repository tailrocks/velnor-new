//! Exact renderer parity and release invariants for generator publication.

use std::collections::BTreeSet;

use velnor_actions_contract::RoutingWorkflow;
use velnor_actions_workflow_renderer::{
    RenderedTree, Schema2WorkflowRequest, render_schema2_workflows,
};

const GENERATOR_RELEASE_PATH: &str = ".github/workflows/generator-release.yml";

/// Render the release workflow directly for byte comparisons.
fn rendered_release() -> Result<String, Box<dyn std::error::Error>> {
    let request = Schema2WorkflowRequest {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::GeneratorRelease]),
        mbx_qualification: None,
    };
    render_schema2_workflows(&request)?
        .into_iter()
        .find(|file| file.path == GENERATOR_RELEASE_PATH)
        .map(|file| file.bytes)
        .ok_or_else(|| format!("renderer omitted {GENERATOR_RELEASE_PATH}").into())
}

/// Staged `.github` rendering matches the generator-release renderer.
pub(super) fn assert_rendered(tree: &RenderedTree) -> Result<(), Box<dyn std::error::Error>> {
    let body = tree
        .get(GENERATOR_RELEASE_PATH)
        .ok_or("missing .github/workflows/generator-release.yml")?;
    assert_eq!(body, rendered_release()?);
    assert_generator(body)
}

/// Committed generator-release workflow matches exact renderer bytes.
pub(super) fn assert_committed(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let path = root.join(GENERATOR_RELEASE_PATH);
    let body = std::fs::read_to_string(&path)?;
    assert_eq!(body, rendered_release()?, "{}", path.display());
    assert_generator(&body)
}

fn assert_generator(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    assert!(body.contains("name: Generator release\n"), "{body}");
    assert_eq!(
        super::job_ids(body),
        vec![
            "release-eligibility",
            "build-linux",
            "attest-linux",
            "build-macos",
            "attest-macos",
            "build-manifest",
            "attest-manifest",
            "publish-generator",
        ]
    );
    assert_eq!(body.matches("contents: write").count(), 1, "{body}");
    assert_eq!(body.matches("id-token: write").count(), 3, "{body}");
    assert_eq!(body.matches("actions/attest-build-provenance@").count(), 3);
    assert_eq!(body.matches("actions/upload-artifact@").count(), 6);
    assert!(body.contains("workflow_dispatch: {}"), "{body}");
    assert!(
        body.contains(
            "generator-0.1.1-linux-assets-${{ github.run_id }}-${{ github.run_attempt }}"
        )
    );
    assert!(
        body.contains(
            "generator-0.1.1-macos-assets-${{ github.run_id }}-${{ github.run_attempt }}"
        )
    );
    assert!(body.contains(
        "generator-0.1.1-manifest-assets-${{ github.run_id }}-${{ github.run_attempt }}"
    ));
    assert!(body.contains("actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"));
    assert!(body.contains("jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5"));
    assert!(body.contains("mr-boxington@1.21.1"));
    assert!(body.contains("gh@2.102.0"));
    assert!(body.contains("cache_save: \"false\""));
    assert!(body.contains("persist-credentials: \"false\""));
    assert!(body.contains("token: \"\""));
    assert!(body.contains("VELNOR_RELEASE_VERSION: 0.1.1"));
    assert!(body.contains("VELNOR_RELEASE_TAG: v0.1.1"));
    assert!(body.contains("velnor-actions-release-manifest.json"));
    assert!(body.contains("velnor-actions-0.1.1-linux-provenance.sigstore.json"));
    assert!(body.contains("velnor-actions-0.1.1-macos-provenance.sigstore.json"));
    assert!(body.contains("velnor-actions-0.1.1-manifest-provenance.sigstore.json"));
    assert!(!body.contains("actions/cache@"), "{body}");
    assert!(!body.contains("--save-cache"), "{body}");
    let linux = super::job_body(body, "build-linux")?;
    let macos = super::job_body(body, "build-macos")?;
    assert!(linux.contains("velnor-actions-0.1.1-x86_64-unknown-linux-gnu"));
    assert!(linux.contains(
        "mbx build --release --locked --package velnor-actions-cli --bin velnor-actions"
    ));
    assert!(linux.contains("ELF*x86-64"));
    assert!(macos.contains("velnor-actions-0.1.1-aarch64-apple-darwin"));
    assert!(macos.contains("Mach-O*arm64"));
    let eligibility = super::job_body(body, "release-eligibility")?;
    assert!(eligibility.contains("bash scripts/generator-release/check-source.sh"));
    assert!(eligibility.contains("Required CI"));
    let publish = super::job_body(body, "publish-generator")?;
    assert!(publish.contains("contents: write"), "{publish}");
    assert!(
        publish.contains("bash scripts/generator-release/publish.sh"),
        "{publish}"
    );
    assert!(!publish.contains("cargo build"), "{publish}");
    assert!(!publish.contains("velnor-actions --version"), "{publish}");
    Ok(())
}
