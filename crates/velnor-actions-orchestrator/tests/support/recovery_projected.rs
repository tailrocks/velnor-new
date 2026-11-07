// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
#[ignore = "requires isolated Linux systemd and a pinned tool archive"]
fn projected_toolchain_installed_verifier_accepts_real_archive()
-> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, Instant};
    let boot = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
    let deadline =
        (u64::try_from(boot.tv_sec)? + 300) * 1_000_000_000 + u64::try_from(boot.tv_nsec)?;
    let custody = crate::release::crate_release_custody::CustodiedChannel::take(deadline)?;
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
    let projected = crate::release::crate_release_projected_tools::ProjectedToolchain::project(
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
