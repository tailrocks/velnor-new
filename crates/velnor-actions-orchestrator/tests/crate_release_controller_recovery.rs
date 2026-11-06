// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Command, GitHubAdmissionRequest, Github, Path, Registry, exercise_repair, fixture_archive,
    fixture_no_baseline, fs, make_fixture, prepare_crate_release, reconcile_crate_release,
    test_git, velnor_actions_orchestrator, verify_crate_release_admission,
};

#[path = "crate_release_controller_recovery_receipts.rs"]
mod receipts;
use receipts::{
    check_authentication_denials, check_receipt_denials, check_receipt_revalidation,
    check_transient_repository, receipt_zip,
};

#[test]
pub(super) fn recovery_requires_fresh_human_approval_and_registry_absence()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    let receipt = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let verify = |candidate: &GitHubAdmissionRequest| {
        verify_crate_release_admission(
            &receipt,
            &intent,
            candidate,
            &github,
            &registry,
            crate::CrateSource::new(root, test_git(), &crate::UnprivilegedSourceExecution),
        )
    };
    let reconcile = || {
        reconcile_crate_release(
            &receipt,
            &intent,
            &request,
            &github,
            &registry,
            crate::CrateSource::new(root, test_git(), &crate::UnprivilegedSourceExecution),
        )
    };
    assert!(verify(&request).is_err());
    github.approved.set(true);
    let mut wrong_workflow = request.clone();
    wrong_workflow.workflow_sha = "cccccccccccccccccccccccccccccccccccccccc".into();
    assert!(verify(&wrong_workflow).is_err());
    let mut wrong_source = request.clone();
    wrong_source.source_sha = "dddddddddddddddddddddddddddddddddddddddd".into();
    assert!(verify(&wrong_source).is_err());
    github.owner_match.set(false);
    assert!(verify(&request).is_err());
    github.owner_match.set(true);
    verify(&request)?;
    registry.absent.set(false);
    assert!(verify(&request).is_err());
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    exercise_repair(&intent, &request, &github, &registry, root)?;
    let completed = reconcile()?;
    assert_eq!(completed[0].release_id, 12);
    assert_eq!(
        completed[0].release_url,
        "https://github.com/example/demo/releases/tag/demo-v0.1.1"
    );
    assert_eq!(completed[0].tag_commit, request.source_sha);
    assert!(completed[0].source_inventory.contains_key("src/lib.rs"));
    for variant in [1, 2, 3] {
        github.release_variant.set(variant);
        assert!(reconcile().is_err());
    }
    github.release_variant.set(4);
    assert!(
        velnor_actions_orchestrator::crate_release_github::read_github_release_evidence(
            "example/demo",
            "demo-v0.1.1",
            &github
        )?
        .is_none()
    );
    github.release_variant.set(0);
    registry.archive.borrow_mut().push(0);
    assert!(reconcile().is_err());
    Ok(())
}

#[test]
pub(super) fn source_free_github_gate_does_not_execute_cargo()
-> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("VELNOR_SOURCE_FREE_GATE").is_some() {
        let temp = tempfile::tempdir()?;
        let (_intent, request, github, _registry) = make_fixture(temp.path())?;
        github.approved.set(true);
        let candidate = crate::crate_release_github::verify_github_evidence(&request, &github)?;
        crate::crate_release_github::authorize_github_candidate(&request, &candidate, &github)?;
        github.approved.set(false);
        assert!(
            crate::crate_release_github::authorize_github_candidate(&request, &candidate, &github)
                .is_err()
        );
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let status = Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("crate_release_controller::tests::source_free_github_gate_does_not_execute_cargo")
        .env("VELNOR_SOURCE_FREE_GATE", "1")
        .env("RUSTUP_HOME", temp.path().join("missing-rustup"))
        .env("MISE_RUSTUP_HOME", temp.path().join("missing-rustup"))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
pub(super) fn receipt_acquire_requires_current_approval_and_matching_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_tool_artifact::{
        ToolchainStageRequest, fixture_acquire_receipt as acquire_receipt_artifact,
    };
    let source = tempfile::tempdir()?;
    let runner = tempfile::tempdir()?;
    let runner_path = runner.path().canonicalize()?;
    let (intent, request, github, registry) = make_fixture(source.path())?;
    github.approved.set(true);
    github.artifact_receipt.set(true);
    let mut receipt = velnor_actions_orchestrator::crate_release_controller::prepare_crate_release(
        &intent,
        &request,
        &github,
        &registry,
        crate::CrateSource::new(
            source.path(),
            test_git(),
            &crate::UnprivilegedSourceExecution,
        ),
    )?;
    receipt.semver_evidence = receipt
        .packages
        .iter()
        .map(|p| (p.source.name.clone(), fixture_no_baseline()))
        .collect();
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec(&receipt)?)?;
    let stage = ToolchainStageRequest {
        intent: &intent,
        github: &request,
        candidate: &receipt.github,
        artifact_id: 10,
        token: "test",
        runner_temp: &runner_path,
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
    };
    let authenticated =
        crate::crate_release_tool_artifact::fixture_authenticate_receipt(&stage, &github, &github)?;
    assert_eq!(authenticated, receipt);
    check_transient_repository(&stage, &github);
    check_authentication_denials(&stage, &github, &receipt)?;
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec(&receipt)?)?;
    assert!(
        !runner_path
            .join("velnor-crate-release-receipt.json")
            .exists()
    );
    github.approved.set(false);
    assert!(
        crate::crate_release_tool_artifact::fixture_authenticate_receipt(&stage, &github, &github,)
            .is_err()
    );
    github.approved.set(true);
    acquire_receipt_artifact(
        &stage,
        &github,
        &registry,
        &github,
        crate::CrateSource::new(
            source.path(),
            test_git(),
            &crate::UnprivilegedSourceExecution,
        ),
    )?;
    assert_eq!(
        serde_json::from_slice::<
            velnor_actions_orchestrator::crate_release_controller::CrateReleaseAdmissionReceipt,
        >(&fs::read(
            runner.path().join("velnor-crate-release-receipt.json")
        )?)?,
        receipt
    );
    fs::remove_file(runner.path().join("velnor-crate-release-receipt.json"))?;
    check_receipt_revalidation(&stage, &github, &registry, &receipt, source.path())?;
    check_receipt_denials(&stage, &github, &registry, &receipt, source.path())
}

#[cfg(unix)]
#[test]
pub(super) fn ambient_rust_wrappers_cannot_override_verified_tools()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir()?;
    let wrapper = temp.path().join("hostile-wrapper");
    fs::write(&wrapper, "#!/bin/sh\nexit 86\n")?;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))?;
    let status = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "crate_release_controller::tests::recovery_requires_fresh_human_approval_and_registry_absence",
        ])
        .env("RUSTC_WRAPPER", &wrapper)
        .env("RUSTC_WORKSPACE_WRAPPER", &wrapper)
        .env("RUSTC", &wrapper)
        .env("RUSTDOC", &wrapper)
        .env("CARGO_ENCODED_RUSTFLAGS", "--definitely-invalid")
        .status()?;
    assert!(status.success());
    Ok(())
}

#[cfg(unix)]
#[test]
pub(super) fn toolchain_staging_requires_approved_same_run_artifact()
-> Result<(), Box<dyn std::error::Error>> {
    use flate2::{Compression, write::GzEncoder};
    use std::io::{Cursor, Write};
    use velnor_actions_orchestrator::crate_release_github::verify_github_evidence;
    use velnor_actions_orchestrator::crate_release_tool_artifact::{
        ToolchainStageRequest, stage_unverified_toolchain,
    };
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("source");
    fs::create_dir(&source)?;
    let (intent, request, github, _) = make_fixture(&source)?;
    github.approved.set(true);
    let candidate = verify_github_evidence(&request, &github)?;
    let host = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut header = tar::Header::new_gnu();
    header.set_size(1);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(
        &mut header,
        format!("toolchains/{}-{host}/bin/cargo", intent.rust_version),
        &b"x"[..],
    )?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "rust-toolchain.tar.gz",
        zip::write::SimpleFileOptions::default(),
    )?;
    zip.write_all(&tar.into_inner()?.finish()?)?;
    *github.tool_archive.borrow_mut() = zip.finish()?.into_inner();
    let runner = temp.path().canonicalize()?.join("runner");
    fs::create_dir_all(runner.join("velnor"))?;
    let stage = ToolchainStageRequest {
        intent: &intent,
        github: &request,
        candidate: &candidate,
        artifact_id: 10,
        token: "fixture-token",
        runner_temp: &runner,
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
    };
    github.mutate_after_download.set(1);
    assert!(stage_unverified_toolchain(&stage, &github, &github).is_err());
    assert!(!runner.join("velnor/rustup").exists());
    github.approved.set(true);
    github.mutate_after_download.set(2);
    assert!(stage_unverified_toolchain(&stage, &github, &github).is_err());
    assert!(!runner.join("velnor/rustup").exists());
    github.tool_archive.borrow_mut().pop();
    github.mutate_after_download.set(0);
    let result = stage_unverified_toolchain(&stage, &github, &github)?;
    assert_eq!(result, runner.join("velnor/rustup"));
    assert_eq!(
        fs::read(result.join(format!(
            "toolchains/{}-{host}/bin/cargo",
            intent.rust_version
        )))?,
        b"x"
    );
    creation_staging::check_creation_staging(&intent, &request, &github, &stage, &result)?;
    Ok(())
}

#[cfg(unix)]
#[path = "crate_release_controller_creation_staging.rs"]
mod creation_staging;
