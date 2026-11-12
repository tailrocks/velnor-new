// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{find_file, job, materialized_script, rendered};
use std::error::Error;

#[test]
fn runtime_bootstrap_publishes_verified_targets_without_executing_them()
-> Result<(), Box<dyn Error>> {
    let files = rendered()?;
    let workflow = find_file(&files, ".github/workflows/source-preview-runtime.yml")?;
    let verify = job(workflow, "verify")?;
    assert!(verify.contains("runs-on: ubuntu-26.04"));
    assert!(verify.contains("actions: read"));
    assert!(verify.contains("contents: read"));
    assert!(!verify.contains("id-token:"));
    assert!(!verify.contains("attestations: write"));
    assert!(!verify.contains("packages: write"));
    assert!(!verify.contains("actions/checkout@"));
    for target in ["linux", "darwin"] {
        assert!(verify.contains(&format!("verified-runtime-sdk-{target}-")));
        assert!(verify.contains(&format!("{target}_artifact_id")));
        assert!(verify.contains(&format!("{target}_archive_digest")));
        assert!(verify.contains(&format!("{target}_sha256")));
    }
    let script = materialized_script(verify)?;
    assert!(script.contains("source_preview_runtime"));
    assert!(!script.contains("source-preview-runtime/linux --"));
    assert!(!script.contains("source-preview-runtime/darwin --"));
    Ok(())
}

#[test]
fn mac_proof_executes_verified_sdk_in_read_only_job() -> Result<(), Box<dyn Error>> {
    let files = rendered()?;
    let workflow = find_file(&files, ".github/workflows/source-preview-mac-proof.yml")?;
    let verify = job(workflow, "verify")?;
    assert!(verify.contains("runs-on: ubuntu-26.04"));
    assert!(verify.contains("actions: read") && verify.contains("contents: read"));
    for forbidden in [
        "id-token:",
        "attestations: write",
        "packages: write",
        "actions/checkout@",
    ] {
        assert!(
            !verify.contains(forbidden),
            "unexpected authority: {forbidden}"
        );
    }
    let expanded = crate::bounded_test_support::expose_materialized_sources(verify)?;
    let acquisition = expanded
        .find("Verify protected runtime SDK")
        .ok_or("missing acquisition")?;
    let assets = expanded
        .find("Download exact release assets")
        .ok_or("missing assets")?;
    let proof = expanded
        .find("Verify production Mac qualification")
        .ok_or("missing proof")?;
    assert!(acquisition < assets && assets < proof);
    for binding in [
        "job.workflow_repository",
        "job.workflow_file_path",
        "job.workflow_ref",
        "job.workflow_sha",
        "job.check_run_id",
    ] {
        assert!(expanded.contains(binding), "missing Q identity: {binding}");
    }
    assert!(expanded.contains("memfd_create") && expanded.contains("F_ADD_SEALS"));
    for output in [
        "manifest_digest",
        "checksums_digest",
        "license_digest",
        "notice_digest",
    ] {
        assert!(verify.contains(&format!("steps.verify_mac.outputs.{output}")));
    }
    Ok(())
}
