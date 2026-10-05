//! Exact schema-2 generator-release workflow body, without the generator marker.

use velnor_actions_workflow_renderer::RenderedTree;

pub(super) const GENERATOR_RELEASE: &str = include_str!("snapshots/generator-release.yml");

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
    assert!(body.contains("name: Generator release\n"), "{body}");
    assert_job_order(body);
    assert_global_policy(body);
    assert_source_gate(body);
    assert_asset_catalog(body);
    assert_target_builds(body)?;
    assert_manifest_job(body)?;
    assert_publish_job(body)?;
    Ok(())
}

fn assert_job_order(body: &str) {
    assert_eq!(
        super::job_ids(body),
        vec![
            "verify-release-source",
            "build-linux",
            "attest-linux",
            "build-macos",
            "attest-macos",
            "build-macos-intel",
            "attest-macos-intel",
            "attest-manifest",
            "publish-generator",
        ]
    );
}

fn assert_global_policy(body: &str) {
    assert_eq!(body.matches("contents: write").count(), 1, "{body}");
    assert_eq!(body.matches("id-token: write").count(), 4, "{body}");
    assert!(!body.contains("v0.1.0"), "{body}");
    assert!(body.contains("generator-${GITHUB_SHA}"), "{body}");
    assert!(body.contains("workflow_dispatch: {}"), "{body}");
    assert!(!body.contains("inputs:"), "{body}");
}

fn assert_source_gate(body: &str) {
    assert!(body.contains("refs/heads/main"), "{body}");
    assert!(body.contains("actions/workflows/ci.yml/runs"), "{body}");
    assert!(body.contains("Required"), "{body}");
    assert!(body.contains("bash scripts/check-freshness.sh"), "{body}");
    assert!(
        !body.contains("check-freshness.sh --check-upstream"),
        "{body}"
    );
    assert!(body.contains("git rev-parse HEAD"), "{body}");
}

fn assert_asset_catalog(body: &str) {
    assert!(body.contains("release-manifest.json"), "{body}");
    assert!(
        body.contains("cmp release-manifest.json manifest-assets/release-manifest.json"),
        "{body}"
    );
    assert!(body.contains("sha256sum --check"), "{body}");
    for binary in [
        "velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
        "velnor-actions-0.1.0-aarch64-apple-darwin",
        "velnor-actions-0.1.0-x86_64-apple-darwin",
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
        "velnor-actions-0.1.0-x86_64-unknown-linux-gnu.provenance.json",
        "velnor-actions-0.1.0-aarch64-apple-darwin.provenance.json",
        "velnor-actions-0.1.0-x86_64-apple-darwin.provenance.json",
    ] {
        assert!(body.contains(provenance), "missing provenance {provenance}");
    }
    assert!(!body.contains("ubuntu-26.04-scale-set"), "{body}");
    assert!(!body.contains("runs-on: [velnor"), "{body}");
    assert!(!body.contains("binary-assets"), "{body}");
    assert!(!body.contains("image-assets"), "{body}");
}

fn assert_target_builds(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    assert_attest(body, "attest-linux", "ubuntu-26.04")?;
    assert_attest(body, "attest-macos", "macos-15")?;
    assert_attest(body, "attest-macos-intel", "macos-15-intel")?;
    assert_candidate_qualification(body, "attest-linux")?;
    assert_candidate_qualification(body, "attest-macos")?;
    assert_candidate_qualification(body, "attest-macos-intel")?;
    let linux = super::job_body(body, "build-linux")?;
    assert!(linux.contains("runs-on: ubuntu-26.04\n"), "{linux}");
    assert!(linux.contains("ELF"), "{linux}");
    assert!(linux.contains("sha256sum"), "{linux}");
    assert!(
        linux.contains(
            "mbx build --release --locked --package velnor-actions-cli --bin velnor-actions"
        ),
        "{linux}"
    );
    assert!(linux.contains("rust@1.98.1 mr-boxington@1.21.1"), "{linux}");
    let macos = super::job_body(body, "build-macos")?;
    assert!(macos.contains("runs-on: macos-15\n"), "{macos}");
    assert!(macos.contains("*Mach-O*arm64*"), "{macos}");
    assert!(macos.contains("shasum -a 256"), "{macos}");
    let intel = super::job_body(body, "build-macos-intel")?;
    assert!(intel.contains("runs-on: macos-15-intel\n"), "{intel}");
    assert!(intel.contains("*Mach-O*x86_64*"), "{intel}");
    assert!(intel.contains("shasum -a 256"), "{intel}");
    Ok(())
}

fn assert_manifest_job(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    assert_attest(body, "attest-manifest", "ubuntu-26.04")?;
    let manifest = super::job_body(body, "attest-manifest")?;
    assert!(manifest.contains("schema\\\":1"), "{manifest}");
    assert!(
        manifest.contains("\\\"version\\\":\\\"0.1.0\\\""),
        "{manifest}"
    );
    assert!(manifest.contains("\\\"commit\\\":\\\"%s\\\""), "{manifest}");
    assert!(manifest.contains("release-manifest.json"), "{manifest}");
    assert!(manifest.contains("toolchain"), "{manifest}");
    assert!(manifest.contains("Attest built artifacts"), "{manifest}");
    assert!(manifest.contains("Upload release manifest"), "{manifest}");
    assert!(manifest.contains("- attest-linux"), "{manifest}");
    assert!(manifest.contains("- attest-macos"), "{manifest}");
    assert!(manifest.contains("- attest-macos-intel"), "{manifest}");
    Ok(())
}

fn assert_publish_job(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    let publish = super::job_body(body, "publish-generator")?;
    assert!(publish.contains("runs-on: ubuntu-26.04\n"), "{publish}");
    assert!(publish.contains("contents: write"), "{publish}");
    assert!(!publish.contains("id-token:"), "{publish}");
    assert!(
        publish.contains("GH_TOKEN: ${{ github.token }}"),
        "{publish}"
    );
    assert!(publish.contains("actions/checkout@"), "{publish}");
    assert!(publish.contains("- attest-linux"), "{publish}");
    assert!(publish.contains("- attest-macos"), "{publish}");
    assert!(publish.contains("- attest-macos-intel"), "{publish}");
    assert!(publish.contains("- attest-manifest"), "{publish}");
    Ok(())
}

fn assert_candidate_qualification(body: &str, id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let job = super::job_body(body, id)?;
    let archive_list = job
        .find("tar -tf \\\"$archive\\\"")
        .ok_or("missing candidate archive member check")?;
    let archive_extract = job
        .find("tar -xf \\\"$archive\\\"")
        .ok_or("missing uploaded candidate archive extraction")?;
    let checksum = job
        .find("Verify downloaded checksum sidecar")
        .ok_or("missing downloaded checksum verification")?;
    let provenance = job
        .find("Verify candidate provenance record")
        .ok_or("missing candidate provenance verification")?;
    let qualification = job
        .find("Qualify downloaded candidate")
        .ok_or("missing exact candidate qualification")?;
    let attestation = job
        .find("Attest built artifacts")
        .ok_or("missing candidate attestation")?;
    assert!(
        archive_list < archive_extract
            && archive_extract < checksum
            && checksum < provenance
            && provenance < qualification
            && qualification < attestation,
        "{job}"
    );
    assert!(job.contains("test -x "), "{job}");
    assert!(job.contains("toolchain"), "{job}");
    assert!(job.contains("--version"), "{job}");
    assert!(job.contains(" generate --output-dir"), "{job}");
    assert!(job.contains("diff -r --brief .github"), "{job}");
    Ok(())
}

fn assert_attest(body: &str, id: &str, runs_on: &str) -> Result<(), Box<dyn std::error::Error>> {
    let job = super::job_body(body, id)?;
    assert!(job.contains(&format!("runs-on: {runs_on}\n")), "{job}");
    assert!(job.contains("id-token: write"), "{job}");
    assert!(!job.contains("contents: write"), "{job}");
    assert!(job.contains("provenance.json"), "{job}");
    Ok(())
}
