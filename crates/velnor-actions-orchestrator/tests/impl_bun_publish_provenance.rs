//! `[publish] provenance` over `[[publish.npm]]`: the default attestation
//! store and the declared Sigstore bundle with its record job.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_bun::{UI_PUBLISHABLE, npm_config, write_publishable};
use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};

#[test]
fn npm_default_provenance_uses_the_attestation_store_and_adds_no_record_job() -> TestResult {
    let repo = make_repo(&npm_config(""))?;
    write_publishable(repo.path(), UI_PUBLISHABLE)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let publish = required_file(&tree, ".github/workflows/publish.yml")?;
    let attest = job_body(publish, "npm-attest-ui")?;
    assert!(attest.contains("attest-build-provenance"), "{attest}");
    assert!(attest.contains("attestations: write"), "{attest}");
    assert!(!publish.contains("npm-provenance-ui"), "{publish}");
    assert!(!publish.contains("cosign"), "{publish}");
    let push = job_body(publish, "npm-publish-ui")?;
    assert!(push.contains("- npm-attest-ui"), "{push}");
    Ok(())
}

#[test]
fn npm_sigstore_bundle_records_verified_provenance_before_the_push() -> TestResult {
    let config = npm_config("").replace(
        "[[publish.npm]]",
        "[publish]\nprovenance = \"sigstore_bundle\"\n[[publish.npm]]",
    );
    let repo = make_repo(&config)?;
    write_publishable(repo.path(), UI_PUBLISHABLE)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let publish = required_file(&tree, ".github/workflows/publish.yml")?;
    let sign = job_body(publish, "npm-attest-ui")?;
    assert!(sign.contains("contents: read"), "{sign}");
    assert!(sign.contains("id-token: write"), "{sign}");
    assert!(sign.contains("attestations: write"), "{sign}");
    assert!(!sign.contains("attest-build-provenance"), "{sign}");
    assert!(!sign.contains("actions/checkout@"), "{sign}");
    assert!(
        sign.contains("attest-blob --yes --signing-config"),
        "{sign}"
    );
    assert!(sign.contains("name: npm-provenance-ui"), "{sign}");
    let record = job_body(publish, "npm-provenance-ui")?;
    assert!(record.contains("- npm-attest-ui"), "{record}");
    assert!(record.contains("contents: write"), "{record}");
    assert!(record.contains("environment: release"), "{record}");
    assert!(!record.contains("packages:"), "{record}");
    assert!(!record.contains("actions/checkout@"), "{record}");
    assert!(record.contains("gh attestation verify"), "{record}");
    assert!(
        record.contains("chainargos-ui-$ver.tgz.sigstore.json")
            && record.contains("chainargos-ui-$ver.tgz.provenance.json"),
        "{record}"
    );
    let push = job_body(publish, "npm-publish-ui")?;
    assert!(push.contains("- npm-provenance-ui"), "{push}");
    assert!(push.contains("packages: write"), "{push}");
    assert!(!push.contains("contents: write"), "{push}");
    let receipt = push.find("Check provenance receipt").ok_or("receipt")?;
    let put = push.find("Publish to GitHub Packages").ok_or("put")?;
    assert!(receipt < put, "{push}");
    Ok(())
}
