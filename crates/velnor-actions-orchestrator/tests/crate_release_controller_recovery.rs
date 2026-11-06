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
    for variant in [1, 2, 3, 10] {
        github.release_variant.set(variant);
        assert!(reconcile().is_err());
    }
    github.release_variant.set(4);
    assert!(
        velnor_actions_orchestrator::crate_release_github::read_github_release_evidence(
            "example/demo",
            "demo-v0.1.1",
            "0.1.1",
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
    use velnor_actions_orchestrator::crate_release_tool_artifact::ToolchainStageRequest;
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
    check_transient_repository(&stage, &github, false);
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
    check_receipt_acquisition(&stage, &github, &registry, &receipt, source.path())
}

fn check_receipt_acquisition(
    stage: &crate::crate_release_tool_artifact::ToolchainStageRequest<'_>,
    github: &Github,
    registry: &Registry,
    receipt: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    source: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_tool_artifact::fixture_acquire_receipt as acquire_receipt_artifact;
    acquire_receipt_artifact(
        stage,
        github,
        registry,
        github,
        crate::CrateSource::new(source, test_git(), &crate::UnprivilegedSourceExecution),
    )?;
    assert_eq!(
        serde_json::from_slice::<
            velnor_actions_orchestrator::crate_release_controller::CrateReleaseAdmissionReceipt,
        >(&fs::read(
            stage.runner_temp.join("velnor-crate-release-receipt.json")
        )?)?,
        *receipt
    );
    fs::remove_file(stage.runner_temp.join("velnor-crate-release-receipt.json"))?;
    check_receipt_revalidation(stage, github, registry, receipt, source)?;
    check_receipt_denials(stage, github, registry, receipt, source)
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
    check_transient_repository(&stage, &github, true);
    staging_postcheck::check_post_extraction(
        &stage,
        &github,
        stage.runner_temp.parent().ok_or("staging parent")?,
    )?;
    let result = stage_unverified_toolchain(&stage, &github, &github)?;
    assert_eq!(result.path(), runner.join("velnor/rustup"));
    assert_eq!(result.intent(), &intent);
    assert_eq!(result.authorization().candidate(), &candidate);
    assert_eq!(result.artifact().id, stage.artifact_id);
    assert_eq!(result.artifact().workflow_run.id, request.run_id);
    assert_eq!(
        result.artifact().workflow_run.head_sha,
        request.workflow_sha
    );
    assert_eq!(
        fs::read(result.path().join(format!(
            "toolchains/{}-{host}/bin/cargo",
            intent.rust_version
        )))?,
        b"x"
    );
    creation_staging::check_creation_staging(&intent, &request, &github, &stage, result.path())?;
    Ok(())
}

#[cfg(unix)]
#[path = "crate_release_controller_creation_staging.rs"]
mod creation_staging;

#[cfg(unix)]
#[path = "crate_release_controller_staging_postcheck.rs"]
mod staging_postcheck;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
#[ignore = "requires isolated Linux systemd and a pinned tool archive"]
fn projected_toolchain_installed_verifier_accepts_real_archive()
-> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, Instant};
    let boot = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
    let deadline =
        (u64::try_from(boot.tv_sec)? + 300) * 1_000_000_000 + u64::try_from(boot.tv_nsec)?;
    let custody = crate::crate_release_custody::CustodiedChannel::take(deadline)?;
    let fixture = tempfile::tempdir_in(custody.root.path())?;
    let source = fixture.path().join("source");
    fs::create_dir(&source)?;
    let (intent, request, github, _) = make_fixture(&source)?;
    github.approved.set(true);
    *github.tool_archive.borrow_mut() =
        fs::read(std::env::var_os("VELNOR_TEST_TOOL_ARCHIVE").ok_or("missing fixture archive")?)?;
    let candidate = crate::crate_release_github::verify_github_evidence(&request, &github)?;
    let stage = crate::crate_release_tool_artifact::ToolchainStageRequest {
        intent: &intent,
        github: &request,
        candidate: &candidate,
        artifact_id: 10,
        token: "fixture-token",
        runner_temp: custody.root.path(),
        deadline: Instant::now() + Duration::from_secs(270),
    };
    fs::create_dir(custody.root.path().join("velnor"))?;
    let staged =
        crate::crate_release_tool_artifact::stage_unverified_toolchain(&stage, &github, &github)?;
    let approved = staged.authorization().clone();
    let projected = crate::crate_release_projected_tools::ProjectedToolchain::project(
        &custody.root,
        &custody.view,
        staged,
        &approved,
        &intent,
    )?;
    projected.revalidate_view(&custody.view)?;
    assert_eq!(projected.cargo_sha256().len(), 64);
    assert_eq!(projected.rustc_sha256().len(), 64);
    assert_eq!(projected.rustdoc_sha256().len(), 64);
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
#[ignore = "requires isolated systemd and real pinned tool fixtures"]
fn sealed_source_adapter_runs_real_supervised_package() -> Result<(), Box<dyn std::error::Error>> {
    use crate::CrateSourceExecution as _;
    use crate::crate_release_controller::{
        CrateReleaseAction, CrateReleaseAdmissionReceipt, CrateReleasePlannedPackage,
    };
    use crate::crate_release_tool_artifact::{
        ToolchainStageRequest,
        receipt::{AuthenticatedSource, acquire_authenticated_receipt},
    };
    use std::{
        os::unix::fs::{PermissionsExt as _, symlink},
        time::{Duration, Instant},
    };
    let clock = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
    let boot_deadline =
        (u64::try_from(clock.tv_sec)? + 480) * 1_000_000_000 + u64::try_from(clock.tv_nsec)?;
    let custody = crate::crate_release_custody::CustodiedChannel::take(boot_deadline)?;
    let fixture = tempfile::tempdir_in(custody.root.path())?;
    let original = fixture.path().join("source-original");
    fs::create_dir(&original)?;
    let (mut intent, request, github, registry) = make_fixture(&original)?;
    let mut previous_intent = intent.clone();
    previous_intent.runner = "ubuntu-24.04".into();
    fs::create_dir_all(original.join(".github"))?;
    fs::write(
        original.join(".github/velnor-release-intent.toml"),
        toml::to_string(&previous_intent)?,
    )?;
    super::git(&original, &["add", "-A"])?;
    super::git(&original, &["commit", "-q", "-s", "-m", "prior policy"])?;
    let previous_sha = super::git(&original, &["rev-parse", "HEAD"])?;
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: previous_sha.clone(),
    });
    github.approved.set(true);
    let candidate = crate::crate_release_github::verify_github_evidence(&request, &github)?;
    let package = crate::CrateReleaseSourcePackage {
        name: "demo".into(),
        version: "0.1.1".into(),
        manifest: "Cargo.toml".into(),
    };
    let receipt = CrateReleaseAdmissionReceipt {
        schema: 1,
        intent: intent.clone(),
        github: candidate.clone(),
        packages: vec![CrateReleasePlannedPackage {
            source: package.clone(),
            tag: "demo-v0.1.1".into(),
            owners: crate::crate_release_registry::CrateRegistryOwnership::Existing {
                owners: vec![crate::crate_release_registry::CrateRegistryOwner {
                    id: 370532,
                    login: "owner".into(),
                    kind: "user".into(),
                }],
            },
            action: CrateReleaseAction::Publish,
        }],
        semver_evidence: std::collections::BTreeMap::from([("demo".into(), fixture_no_baseline())]),
    };
    github.artifact_receipt.set(true);
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec(&receipt)?)?;
    let stage = ToolchainStageRequest {
        intent: &intent,
        github: &request,
        candidate: &candidate,
        artifact_id: 10,
        token: "fixture-token",
        runner_temp: custody.root.path(),
        deadline: Instant::now() + Duration::from_secs(440),
    };
    let authenticated = acquire_authenticated_receipt(&stage, &github, &github)?;
    github.artifact_receipt.set(false);
    let git_dir = custody.root.path().join("git-fixture");
    fs::create_dir(&git_dir)?;
    for (variable, name) in [
        ("VELNOR_TEST_PINNED_GIT", "git"),
        ("VELNOR_TEST_PINNED_GIT_HTTP", "git-remote-http"),
    ] {
        fs::copy(
            std::env::var_os(variable).ok_or("missing pinned git fixture")?,
            git_dir.join(name),
        )?;
        fs::set_permissions(git_dir.join(name), fs::Permissions::from_mode(0o555))?;
    }
    symlink("git-remote-http", git_dir.join("git-remote-https"))?;
    let git = crate::crate_release_git::ReleaseGit::new(
        velnor_actions_mise::release_git_pin::verify_bundle(&git_dir)?,
    );
    let budget = velnor_actions_mise::CheckDeadline::after(Duration::from_secs(420))?;
    let snapshot = AuthenticatedSource::snapshot(authenticated, &original, &git, budget)?;
    let checkout = custody.view.path().join("source");
    let status = std::process::Command::new("/usr/bin/git")
        .args(["clone", "--quiet", "--no-hardlinks", "--local"])
        .arg(&original)
        .arg(&checkout)
        .status()?;
    if !status.success() {
        return Err("fixture clone failed".into());
    }
    let status = std::process::Command::new("/usr/bin/git")
        .args(["-C"])
        .arg(&checkout)
        .args(["checkout", "--quiet", "--detach", &request.source_sha])
        .status()?;
    if !status.success() {
        return Err("fixture checkout failed".into());
    }
    fs::create_dir(custody.root.path().join("velnor"))?;
    fs::create_dir(custody.root.path().join("velnor-semver"))?;
    fs::create_dir(custody.root.path().join("velnor-release-plz"))?;
    fs::copy(
        std::env::var_os("VELNOR_TEST_RELEASE_PLZ").ok_or("missing release-plz fixture")?,
        custody.root.path().join("velnor-release-plz/release-plz"),
    )?;
    fs::copy(
        std::env::var_os("VELNOR_TEST_SEMVER").ok_or("missing semver fixture")?,
        custody
            .root
            .path()
            .join("velnor-semver/cargo-semver-checks"),
    )?;
    *github.tool_archive.borrow_mut() =
        fs::read(std::env::var_os("VELNOR_TEST_TOOL_ARCHIVE").ok_or("missing Rust fixture")?)?;
    let staged =
        crate::crate_release_tool_artifact::stage_unverified_toolchain(&stage, &github, &github)?;
    let authorized = staged.authorization().clone();
    let projected = crate::crate_release_projected_tools::ProjectedToolchain::project(
        &custody.root,
        &custody.view,
        staged,
        &authorized,
        &intent,
    )?;
    let view_path = custody.view.path().to_owned();
    let sealed = crate::crate_release_source_view::SealedSourceView::seal(
        snapshot,
        custody.view,
        projected,
        git.clone(),
        &github,
        budget,
    )?;
    let tools = sealed.project_source_tools(&custody.root)?;
    let context: serde_json::Value = serde_json::from_slice(&tools.context(false)?)?;
    let canonical_intent = velnor_actions_contract::canonical_json_bytes(&intent)?;
    assert_eq!(
        context["identity"]["intent_sha256"],
        crate::generator::sha256_hex(&canonical_intent),
        "source session and upload permit must bind the same approved intent"
    );
    if std::env::var("VELNOR_TEST_CUSTODY_FAULT").as_deref() == Ok("expose") {
        let view = view_path;
        let moved = view.with_extension("displaced");
        let result = tools.connect_with_exposure_hook(custody.socket, boot_deadline, || {
            fs::rename(&view, &moved).map_err(|_| "fixture failure")?;
            fs::create_dir(&view).map_err(|_| "fixture failure")?;
            fs::set_permissions(&view, fs::Permissions::from_mode(0o711))
                .map_err(|_| "fixture failure")?;
            Ok(())
        });
        fs::remove_dir(&view)?;
        fs::rename(&moved, &view)?;
        assert!(result.is_err(), "replacement during exposure accepted");
        return Ok(());
    }
    let execution = tools.connect(custody.socket, boot_deadline)?;
    if let Ok(preflight) = std::env::var("VELNOR_TEST_UPLOAD_PREFLIGHT") {
        let result = execution.credential_preflight(execution.root(), &intent, &git);
        if preflight == "missing" {
            assert!(result.is_err(), "missing credential accepted");
            return Ok(());
        }
        result?;
    }
    assert!(
        crate::UnprivilegedSourceExecution
            .historical_intent_reader()
            .is_err()
    );
    let (_, mut prior) =
        super::fixture_journal_plan(execution.root(), &previous_intent, &request, &receipt)?;
    prior.identity.run_id = 11;
    prior.identity.workflow_sha.clone_from(&previous_sha);
    prior.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    let (_, mut current) =
        super::fixture_journal_plan(execution.root(), &intent, &request, &receipt)?;
    current.predecessor = Some(prior.identity.clone());
    let history_api = super::HistoricalAuthority {
        base: &github,
        workflow: previous_sha,
        older: None,
        event: "workflow_dispatch",
        defect: std::cell::Cell::new(0),
    };
    let reader = execution.historical_intent_reader()?;
    if let Ok(fault) = std::env::var("VELNOR_TEST_CUSTODY_FAULT") {
        let journal = super::JournalRegistryFake::default();
        match fault.as_str() {
            "source" => {
                let source = execution.root();
                let moved = source.with_extension("displaced");
                fs::rename(source, &moved)?;
                fs::create_dir(source)?;
                fs::set_permissions(source, fs::Permissions::from_mode(0o555))?;
                assert!(
                    execution
                        .verify_toolchain_separation(source, &intent, &git)
                        .is_err(),
                    "source substitution accepted"
                );
                fs::remove_dir(source)?;
                fs::rename(moved, source)?;
            }
            "tool" => {
                let view = execution.root().parent().ok_or("source view")?;
                let cargo = view.join(format!(
                    "rustup/toolchains/{}-x86_64-unknown-linux-gnu/bin/cargo",
                    intent.rust_version
                ));
                let moved = cargo.with_extension("displaced");
                fs::rename(&cargo, &moved)?;
                fs::write(&cargo, b"substituted")?;
                fs::set_permissions(&cargo, fs::Permissions::from_mode(0o555))?;
                assert!(
                    execution
                        .verify_toolchain_separation(execution.root(), &intent, &git)
                        .is_err(),
                    "tool substitution accepted"
                );
                fs::remove_file(&cargo)?;
                fs::rename(moved, cargo)?;
            }
            "history" => {
                let mut forged = prior.clone();
                forged.intent_sha256 = "e".repeat(64);
                assert!(
                    reader
                        .verify_transition(&forged, &current, &request, &history_api)
                        .is_err(),
                    "forged prior intent accepted"
                );
            }
            _ => return Err("unknown fixture fault".into()),
        }
        let generator = crate::package_release_signer::fixture_attested_generator();
        let attempted = crate::package_release_signer::AdmittedStart {
            journal: &journal,
            github: &history_api,
            registry: &registry,
            intent: &intent,
            request: &request,
            admission: &receipt,
            source: execution.root(),
            git: &git,
            source_execution: &execution,
            workflow_root: &original,
            generator: &generator,
        };
        assert!(
            crate::package_release_signer::record_admitted_start(&attempted).is_err(),
            "journal start accepted poisoned custody"
        );
        assert_eq!(
            journal.write_calls.get(),
            0,
            "wrote journal after custody fault"
        );
        assert!(
            execution.historical_intent_reader().is_err(),
            "poisoned session issued a history capability"
        );
        assert!(
            execution.metadata(execution.root(), &intent, &git).is_err(),
            "poisoned session resumed"
        );
        return Ok(());
    }
    reader.verify_transition(&prior, &current, &request, &history_api)?;
    reader.verify_prior(&prior, &request, &history_api)?;
    let journal = super::JournalRegistryFake::default();
    crate::crate_release_journal::record_source_bound_start(
        &journal,
        &prior.identity,
        &prior,
        execution.root(),
        &git,
    )?;
    crate::crate_release_journal::record_journal_intent(
        &journal,
        &prior.identity,
        &prior,
        &crate::crate_release_journal::JournalIntendedMutation {
            semver_evidence: Some(fixture_no_baseline()),
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: crate::crate_release_journal::JournalMutationKind::Upload,
        },
    )?;
    let generator = crate::package_release_signer::fixture_attested_generator();
    let start = crate::package_release_signer::AdmittedStart {
        journal: &journal,
        github: &history_api,
        registry: &registry,
        intent: &intent,
        request: &request,
        admission: &receipt,
        source: execution.root(),
        git: &git,
        source_execution: &execution,
        workflow_root: &original,
        generator: &generator,
    };
    crate::package_release_signer::record_admitted_start(&start)?;
    let started = crate::crate_release_journal::inspect_journal(
        &journal,
        &current.identity,
        crate::crate_release_journal::JournalStage::Started,
    )?
    .ok_or("missing started journal")?;
    let observation = crate::crate_release_controller::CrateReleaseObservation {
        journal: &journal,
        identity: &current.identity,
        journal_receipt: &started.receipt,
        admission: &receipt,
        intent: &intent,
        request: &request,
        github: &history_api,
        registry: &registry,
        source: execution.root(),
        git: &git,
        source_execution: &execution,
        sequence: 1,
    };
    let recovered =
        crate::crate_release_controller::observe_prior_recovery(&observation, &prior.identity)?;
    if recovered.len() != 1 {
        return Err("historical recovery was not observed".into());
    }
    let metadata = execution.metadata(execution.root(), &intent, &git)?;
    if !metadata.contains("\"name\":\"demo\"") {
        return Err("package missing from supervised metadata".into());
    }
    let archive = execution.package(&crate::CratePackageSource::new(
        execution.root(),
        &intent,
        &package,
        fixture.path(),
        &request.source_sha,
        false,
        &git,
    ))?;
    let files = crate::crate_release_archive::read_archive(&archive, "demo-0.1.1/")?;
    if files.get("src/lib.rs").map(|entry| entry.1.as_slice())
        != Some(b"pub fn demo() {}".as_slice())
    {
        return Err("package bytes differ".into());
    }
    if std::env::var_os("VELNOR_TEST_UPLOAD_FULL").is_some() {
        fs::write(custody.root.path().join("expected-demo.crate"), &archive)?;
    }
    let mut baseline = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    for (name, content) in [
        ("Cargo.toml", b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\ndescription = \"fixture\"\nlicense = \"MIT\"\n[features]\ndemo-feature = []\n".as_slice()),
        ("Cargo.lock", b"# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n".as_slice()),
        ("src/lib.rs", b"pub fn demo() {}".as_slice()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(u64::try_from(content.len())?);
        header.set_mode(0o644);
        header.set_cksum();
        baseline.append_data(&mut header, format!("demo-0.1.0/{name}"), content)?;
    }
    let baseline = baseline.into_inner()?.finish()?;
    let checksum = crate::generator::sha256_hex(&baseline);
    let features: Vec<String> = Vec::new();
    execution.check_semver(&crate::CrateSemverSource {
        source: execution.root(),
        rust: &intent.rust_version,
        manifest: &package.manifest,
        package: &package.name,
        baseline_version: "0.1.0",
        baseline_checksum: &checksum,
        baseline_archive: &baseline,
        features: &features,
        executable: &custody
            .root
            .path()
            .join("velnor-semver/cargo-semver-checks"),
        git: &git,
    })?;
    if std::env::var_os("VELNOR_TEST_UPLOAD_FULL").is_some() {
        *registry.baseline.borrow_mut() = baseline;
        execution.credential_preflight(execution.root(), &intent, &git)?;
        let verified = crate::crate_release_semver::verify_before_upload(
            execution.root(),
            &receipt,
            &registry,
            custody.root.path(),
            "demo",
            &git,
            &execution,
        )?;
        let permit = crate::crate_release_controller::verify_and_record_upload_intent(
            &observation,
            "demo",
            verified,
        )?;
        let attempt = crate::crate_release_controller::invoke_supervised_upload(
            &observation,
            permit,
            &execution,
        );
        if std::env::var_os("VELNOR_TEST_UPLOAD_DENY").is_some()
            || std::env::var_os("VELNOR_TEST_UPLOAD_DROP").is_some()
        {
            assert!(attempt.is_err(), "uncertain upload was accepted");
            let mutation = crate::crate_release_journal::inspect_journal_mutation(
                &journal,
                &current.identity,
                1,
            )?
            .ok_or("missing upload intent")?;
            assert!(matches!(
                mutation.phase,
                crate::crate_release_journal::JournalMutationPhase::Intended
            ));
            assert!(
                crate::crate_release_journal::inspect_journal(
                    &journal,
                    &current.identity,
                    crate::crate_release_journal::JournalStage::Completed,
                )?
                .is_none()
            );
            assert!(
                execution
                    .credential_preflight(execution.root(), &intent, &git)
                    .is_err()
            );
            return Ok(());
        }
        let attempt = attempt?;
        assert!(matches!(
            attempt,
            crate::crate_release_controller::UploadAttempt::ExitedSuccess
        ));
    }
    registry.absent.set(false);
    *registry.archive.borrow_mut() = archive;
    github.tag_exists.set(true);
    github.release_variant.set(0);
    crate::crate_release_controller::coordinate_release(
        &observation,
        |_| {
            Err(crate::OrchestratorError::Contract {
                problem: "unexpected semver".into(),
            })
        },
        |_| {
            Err(crate::OrchestratorError::Contract {
                problem: "unexpected upload".into(),
            })
        },
        || {},
    )?;
    let frozen = crate::crate_release_controller::FrozenCompletion::prepare_supervised(
        &observation,
        crate::package_release_signer::fixture_measured_tools(
            started.receipt.expected_tools.clone(),
        ),
        &execution,
    )?;
    execution
        .finish(frozen)?
        .complete(&journal, &history_api, &registry)?;
    Ok(())
}
