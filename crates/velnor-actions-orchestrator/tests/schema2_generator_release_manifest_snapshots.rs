//! Direct generator-release manifest attestation contract checks.

pub(super) fn assert_manifest_job(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    let candidate = assert_candidate_manifest_job(body)?;
    assert_manifest_attestation_job(body)?;
    assert_manifest_builder_contract(candidate);
    Ok(())
}

fn assert_candidate_manifest_job(body: &str) -> Result<&str, Box<dyn std::error::Error>> {
    let candidate = super::super::job_body(body, "candidate-manifest")?;
    assert!(candidate.contains("runs-on: ubuntu-26.04\n"), "{candidate}");
    assert!(candidate.contains("- verify-release-caller"), "{candidate}");
    assert!(candidate.contains("- build-linux"), "{candidate}");
    assert!(candidate.contains("- build-macos"), "{candidate}");
    assert!(candidate.contains("- build-macos-intel"), "{candidate}");
    assert!(candidate.contains("manifest_sha256:"), "{candidate}");
    assert!(candidate.contains("gh_version:"), "{candidate}");
    assert!(
        candidate.contains("Upload same-run release manifest"),
        "{candidate}"
    );
    assert!(
        candidate.contains("manifest-assets/release-manifest.json"),
        "{candidate}"
    );
    Ok(candidate)
}

fn assert_manifest_attestation_job(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_job = super::super::job_body(body, "attest-manifest")?;
    assert!(
        manifest_job.contains("runs-on: ubuntu-26.04\n"),
        "{manifest_job}"
    );
    assert!(manifest_job.contains("id-token: write"), "{manifest_job}");
    assert!(!manifest_job.contains("contents: write"), "{manifest_job}");
    assert!(
        manifest_job.contains("- candidate-manifest"),
        "{manifest_job}"
    );
    for attester in ["attest-linux", "attest-macos", "attest-macos-intel"] {
        assert!(
            manifest_job.contains(&format!("- {attester}\n")),
            "{manifest_job}"
        );
    }
    for build in ["build-linux", "build-macos", "build-macos-intel"] {
        assert!(
            manifest_job.contains(&format!(
                "artifact-ids: ${{{{ needs.{build}.outputs.artifact_id }}}}"
            )),
            "{manifest_job}"
        );
    }
    assert!(
        manifest_job.contains("artifact-ids: ${{ needs.candidate-manifest.outputs.artifact_id }}"),
        "{manifest_job}"
    );
    assert!(
        manifest_job
            .contains(
                "VELNOR_RELEASE_MANIFEST_SHA256: ${{ needs.candidate-manifest.outputs.manifest_sha256 }}"
            ),
        "{manifest_job}"
    );
    assert_eq!(
        manifest_job
            .matches("name: Validate exact downloaded candidate files")
            .count(),
        3,
        "{manifest_job}"
    );
    assert_eq!(
        manifest_job
            .matches("name: Verify source-bound candidate provenance")
            .count(),
        3,
        "{manifest_job}"
    );
    assert!(
        manifest_job.contains("Verify canonical candidate manifest digest"),
        "{manifest_job}"
    );
    assert!(
        manifest_job.contains("Attest built artifacts"),
        "{manifest_job}"
    );
    assert!(
        manifest_job.contains("Upload verified manifest attestation bundle"),
        "{manifest_job}"
    );
    assert!(manifest_job.contains("--signer-digest"), "{manifest_job}");
    assert!(
        manifest_job.contains("actions/download-artifact@"),
        "{manifest_job}"
    );
    Ok(())
}

fn assert_manifest_builder_contract(manifest: &str) {
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        manifest.contains(&format!(
            "create-release-manifest.sh '{version}' 'tailrocks/velnor-new' '1.99.1' '1.27.0'"
        )) || manifest.contains(&format!(
            r#"create-release-manifest.sh '{version}' 'tailrocks/velnor-new' ''\"${{VELNOR_TOOL_RUST}}\"'' ''\"${{VELNOR_TOOL_MR_BOXINGTON}}\"''"#
        )),
        "{manifest}"
    );
    assert!(manifest.contains("release-manifest.json"), "{manifest}");
    let manifest_builder =
        include_str!("../../../scripts/generator-release/create-release-manifest.sh");
    assert!(
        manifest_builder.contains(".schema == 1"),
        "{manifest_builder}"
    );
    assert!(
        manifest_builder.contains(".version == $version"),
        "{manifest_builder}"
    );
    assert!(
        manifest_builder.contains(".commit == $commit"),
        "{manifest_builder}"
    );
    assert!(
        manifest_builder.contains(".toolchain.rust == $rust"),
        "{manifest_builder}"
    );
    assert!(
        manifest_builder.contains(r#".toolchain["mr-boxington"] == $mr_boxington"#),
        "{manifest_builder}"
    );
}
