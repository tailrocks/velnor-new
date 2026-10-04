//! Exact schema-2 generator-release workflow body, without the generator marker.

use velnor_actions_contract::RELEASE_MANIFEST_FILENAME;
use velnor_actions_workflow_renderer::RenderedTree;

pub(super) const GENERATOR_RELEASE: &str = include_str!("schema2_generator_release_snapshot.yml");

/// Byte-lock the rendered workflow and check release invariants.
pub(super) fn assert_rendered(tree: &RenderedTree) -> Result<(), Box<dyn std::error::Error>> {
    let body = tree
        .get(".github/workflows/generator-release.yml")
        .ok_or("missing .github/workflows/generator-release.yml")?;
    assert_eq!(body, super::marked(GENERATOR_RELEASE));
    assert_generator(body)?;
    Ok(())
}

/// Committed `generator-release.yml` matches the rendered bytes.
pub(super) fn assert_committed(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let path = root.join(".github/workflows/generator-release.yml");
    let body = std::fs::read_to_string(&path)?;
    assert_eq!(body, super::marked(GENERATOR_RELEASE), "{}", path.display());
    assert_generator(&body)?;
    Ok(())
}

fn assert_generator(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    assert_workflow_shape(body);
    assert_gate(super::job_body(body, "release-gate")?);
    assert_builds(body)?;
    assert_attest(body, "attest-linux-x64", "build-linux-x64")?;
    assert_attest(body, "attest-macos-arm64", "build-macos-arm64")?;
    assert_attest(body, "attest-macos-x64", "build-macos-x64")?;
    assert_manifest(body)?;
    assert_publish(super::job_body(body, "publish-generator")?)?;
    Ok(())
}

fn assert_workflow_shape(body: &str) {
    assert!(body.contains("name: Generator release\n"), "{body}");
    assert_eq!(
        super::job_ids(body),
        vec![
            "release-gate",
            "build-linux-x64",
            "build-macos-arm64",
            "build-macos-x64",
            "attest-linux-x64",
            "attest-macos-arm64",
            "attest-macos-x64",
            "prepare-manifest",
            "publish-generator",
        ]
    );
    assert!(body.contains("workflow_dispatch: {}"), "{body}");
    assert!(!body.contains("pull_request:"), "{body}");
    assert!(!body.contains("inputs:"), "{body}");
    assert!(!body.contains("gh release create \"v0.1.0\""), "{body}");
    assert!(body.contains("generator-$GITHUB_SHA"), "{body}");
    assert_eq!(body.matches("contents: write").count(), 1, "{body}");
    assert_eq!(body.matches("id-token: write").count(), 4, "{body}");
    assert_eq!(body.matches("attestations: write").count(), 4, "{body}");
    assert_eq!(
        body.matches("artifact-metadata: write").count(),
        4,
        "{body}"
    );
    assert_eq!(body.matches("actions: write").count(), 0, "{body}");
    assert_eq!(body.matches("actions: read").count(), 3, "{body}");
    assert!(
        body.contains("actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"),
        "{body}"
    );
    assert!(
        body.contains("jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5"),
        "{body}"
    );
    assert!(
        body.contains("actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"),
        "{body}"
    );
    assert!(
        body.contains("actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c"),
        "{body}"
    );
    assert!(
        body.contains("actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8"),
        "{body}"
    );
    assert!(!body.contains("ubuntu-26.04-scale-set"), "{body}");
    assert!(!body.contains("runs-on: [velnor"), "{body}");
}

fn assert_builds(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    assert_build(
        super::job_body(body, "build-linux-x64")?,
        "runs-on: ubuntu-26.04\n",
        "x86_64-unknown-linux-gnu",
        "ELF*x86-64",
        "sha256sum",
        "Linux:x86_64",
    );
    assert_build(
        super::job_body(body, "build-macos-arm64")?,
        "runs-on: macos-15\n",
        "aarch64-apple-darwin",
        "Mach-O*arm64",
        "shasum -a 256",
        "macOS:arm64",
    );
    assert_build(
        super::job_body(body, "build-macos-x64")?,
        "runs-on: macos-15\n",
        "x86_64-apple-darwin",
        "Mach-O*x86_64",
        "shasum -a 256",
        "macOS:arm64",
    );
    assert!(
        super::job_body(body, "build-macos-x64")?
            .contains("CARGO_BUILD_TARGET=x86_64-apple-darwin mise"),
        "{}",
        super::job_body(body, "build-macos-x64")?
    );
    Ok(())
}

fn assert_gate(gate: &str) {
    assert!(
        gate.contains("github.event_name == 'workflow_dispatch'"),
        "{gate}"
    );
    assert!(
        gate.contains("github.repository == 'tailrocks/velnor-new'"),
        "{gate}"
    );
    assert!(gate.contains("github.ref == 'refs/heads/main'"), "{gate}");
    assert!(gate.contains("github.ref_protected"), "{gate}");
    assert!(gate.contains("actions: read"), "{gate}");
    assert!(gate.contains("GITHUB_REF_PROTECTED"), "{gate}");
    assert!(gate.contains("git rev-parse HEAD"), "{gate}");
    assert!(gate.contains("GITHUB_SHA"), "{gate}");
    assert!(gate.contains("head_sha == $sha"), "{gate}");
    assert!(
        gate.contains("head_repository.full_name == $repo"),
        "{gate}"
    );
    assert!(
        gate.contains(".path == \\\".github/workflows/ci.yml\\\""),
        "{gate}"
    );
    assert!(gate.contains(".conclusion =="), "{gate}");
    assert!(gate.contains("success"), "{gate}");
    assert!(gate.contains(".default_branch =="), "{gate}");
    assert!(gate.contains("main"), "{gate}");
    assert!(gate.contains(".type =="), "{gate}");
    assert!(gate.contains("required_reviewers"), "{gate}");
    assert!(gate.contains(".prevent_self_review == true"), "{gate}");
    assert!(gate.contains(".reviewers | length"), "{gate}");
    assert!(
        gate.contains(".deployment_branch_policy.protected_branches == true"),
        "{gate}"
    );
    assert!(
        gate.contains(".deployment_branch_policy.custom_branch_policies == false"),
        "{gate}"
    );
    assert!(gate.contains("scripts/check-freshness.sh"), "{gate}");
    assert!(gate.contains("generator-release"), "{gate}");
}

fn assert_build(
    job: &str,
    runner: &str,
    target: &str,
    architecture: &str,
    checksum: &str,
    host: &str,
) {
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        job.contains(&format!("velnor-actions-{version}-{target}")),
        "{job}"
    );
    assert!(job.contains(runner), "{job}");
    assert!(job.contains(target), "{job}");
    assert!(job.contains(architecture), "{job}");
    assert!(job.contains(checksum), "{job}");
    assert!(job.contains(host), "{job}");
    assert!(
        job.contains("catalog_version MR_BOXINGTON_VERSION"),
        "{job}"
    );
    assert!(job.contains("mbx --version"), "{job}");
    assert!(job.contains("mbx stats --json"), "{job}");
    assert!(job.contains(".savings.builds"), "{job}");
    assert!(
        job.contains(
            "mbx build --release --locked --package velnor-actions-cli --bin velnor-actions"
        ),
        "{job}"
    );
    assert!(job.contains("git rev-parse HEAD"), "{job}");
    assert!(job.contains("actions/upload-artifact@"), "{job}");
}

fn assert_attest(body: &str, id: &str, build_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let job = super::job_body(body, id)?;
    assert!(job.contains("runs-on: ubuntu-26.04\n"), "{job}");
    assert!(job.contains("id-token: write"), "{job}");
    assert!(job.contains("attestations: write"), "{job}");
    assert!(job.contains("artifact-metadata: write"), "{job}");
    assert!(!job.contains("actions:"), "{job}");
    assert!(!job.contains("contents: write"), "{job}");
    assert!(job.contains(&format!("- {build_id}")), "{job}");
    assert!(job.contains("actions/attest-build-provenance@"), "{job}");
    Ok(())
}

fn assert_publish(publish: &str) -> Result<(), Box<dyn std::error::Error>> {
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        publish.contains("environment: generator-release\n"),
        "{publish}"
    );
    assert!(publish.contains("contents: write"), "{publish}");
    assert!(publish.contains("attestations: read"), "{publish}");
    assert!(publish.contains("actions: read"), "{publish}");
    assert!(!publish.contains("id-token:"), "{publish}");
    assert!(!publish.contains("attestations: write"), "{publish}");
    assert!(
        publish.contains("GH_TOKEN: ${{ github.token }}"),
        "{publish}"
    );
    assert!(publish.contains("GITHUB_REF_PROTECTED"), "{publish}");
    assert!(publish.contains("verify_same_sha_ci"), "{publish}");
    assert!(publish.contains("verify_release_environment"), "{publish}");
    assert!(publish.contains("assert_files linux-assets"), "{publish}");
    assert!(
        publish.contains("assert_files macos-arm64-assets"),
        "{publish}"
    );
    assert!(
        publish.contains("assert_files macos-x64-assets"),
        "{publish}"
    );
    assert!(publish.contains("verify_sidecar"), "{publish}");
    assert!(publish.contains("sha256sum"), "{publish}");
    assert!(publish.contains("gh attestation verify"), "{publish}");
    assert!(publish.contains("--signer-workflow"), "{publish}");
    assert!(
        publish.contains(".github/workflows/generator-release.yml"),
        "{publish}"
    );
    assert!(publish.contains("--source-digest"), "{publish}");
    assert!(publish.contains("$GITHUB_SHA"), "{publish}");
    assert!(
        publish.contains("--source-ref refs/heads/main"),
        "{publish}"
    );
    assert!(publish.contains("--deny-self-hosted-runners"), "{publish}");
    assert!(publish.contains("git ls-remote"), "{publish}");
    let validate = publish
        .find("validate_assets")
        .ok_or("missing artifact sidecar and attestation validation")?;
    let manifest = publish
        .find(&format!("release-manifest/{RELEASE_MANIFEST_FILENAME}"))
        .ok_or("missing canonical manifest download")?;
    let verify_manifest = publish
        .rfind("verify_attestation")
        .ok_or("missing manifest attestation verification")?;
    let canonical = publish
        .rfind("verify_canonical_manifest")
        .ok_or("missing canonical manifest verification")?;
    let release = publish
        .find("gh release create")
        .ok_or("missing release publication")?;
    assert!(validate < manifest && manifest < verify_manifest && verify_manifest < canonical);
    assert!(canonical < release);
    assert!(publish.contains("schema:1"), "{publish}");
    assert!(
        publish.contains(&format!(
            "velnor-actions-{version}-x86_64-unknown-linux-gnu"
        )),
        "{publish}"
    );
    assert!(publish.contains(RELEASE_MANIFEST_FILENAME), "{publish}");
    assert!(
        publish.contains(&format!("velnor-actions {version} built from")),
        "{publish}"
    );
    assert!(publish.contains("repository:"), "{publish}");
    assert!(publish.contains("tailrocks/velnor-new"), "{publish}");
    assert!(publish.contains("commit:$commit"), "{publish}");
    assert!(publish.contains("targets:"), "{publish}");
    assert!(publish.contains("sha256:$linux_sha"), "{publish}");
    assert!(publish.contains("sha256:$macos_arm_sha"), "{publish}");
    assert!(publish.contains("sha256:$macos_x64_sha"), "{publish}");
    assert!(publish.contains("--latest=false"), "{publish}");
    Ok(())
}

fn assert_manifest(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    let job = super::job_body(body, "prepare-manifest")?;
    assert!(job.contains("attestations: write"), "{job}");
    assert!(job.contains("id-token: write"), "{job}");
    assert!(job.contains("artifact-metadata: write"), "{job}");
    assert!(job.contains("actions: read"), "{job}");
    assert!(job.contains("verify_same_sha_ci"), "{job}");
    assert!(job.contains("verify_release_environment"), "{job}");
    assert!(job.contains("validate_assets"), "{job}");
    assert!(job.contains("verify_canonical_manifest"), "{job}");
    assert!(job.contains("actions/attest-build-provenance@"), "{job}");
    assert!(
        job.contains(&format!("release-manifest/{RELEASE_MANIFEST_FILENAME}")),
        "{job}"
    );
    assert!(job.contains("Upload attested release manifest"), "{job}");
    let validation = job.find("validate_assets").ok_or("missing validation")?;
    let create = job.find("jq -n").ok_or("missing manifest creation")?;
    let attest = job
        .find("Attest built assets")
        .ok_or("missing manifest attestation")?;
    let upload = job
        .find("Upload attested release manifest")
        .ok_or("missing manifest upload")?;
    assert!(validation < create && create < attest && attest < upload);
    Ok(())
}
