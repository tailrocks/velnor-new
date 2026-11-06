// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
#[ignore = "requires isolated systemd and real pinned tool fixtures"]
fn sealed_source_adapter_runs_real_supervised_package() -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_tool_artifact::{ToolchainStageRequest, receipt::AuthenticatedSource};
    use std::time::Duration;
    let SupervisedFixture {
        custody,
        fixture,
        original,
        intent,
        previous_intent,
        previous_sha,
        request,
        github,
        registry,
        package,
        receipt,
        candidate,
        authenticated,
        stage_deadline,
        boot_deadline,
    } = supervised_fixture()?;
    let stage = ToolchainStageRequest {
        intent: &intent,
        github: &request,
        candidate: &candidate,
        artifact_id: 10,
        token: "fixture-token",
        runner_temp: custody.root.path(),
        deadline: stage_deadline,
    };
    github.artifact_receipt.set(false);
    let git = supervised_fixture_git(custody.root.path())?;
    let budget = velnor_actions_mise::CheckDeadline::after(Duration::from_secs(420))?;
    let snapshot = AuthenticatedSource::snapshot(authenticated, &original, &git, budget)?;
    supervised_fixture_checkout(
        &original,
        custody.view.path(),
        custody.root.path(),
        &request.source_sha,
        &github,
    )?;
    let staged =
        crate::crate_release_tool_artifact::stage_unverified_toolchain(&stage, &github, &github)?;
    let authorized = staged.authorization().clone();
    let projected = crate::release::crate_release_projected_tools::ProjectedToolchain::project(
        &custody.root,
        &custody.view,
        staged,
        &authorized,
        &intent,
    )?;
    let view_path = custody.view.path().to_owned();
    let sealed = crate::release::crate_release_source_view::SealedSourceView::seal(
        snapshot,
        custody.view,
        projected,
        git.clone(),
        &github,
        budget,
    )?;
    let tools = sealed.project_source_tools(&custody.root)?;
    let Some(execution) =
        connect_supervised_fixture(tools, view_path, custody.socket, boot_deadline, &intent)?
    else {
        return Ok(());
    };
    supervised_fixture_scenario(SupervisedScenario {
        execution,
        intent: &intent,
        git: &git,
        previous_intent: &previous_intent,
        previous_sha,
        request: &request,
        receipt: &receipt,
        github: &github,
        registry: &registry,
        fixture: &fixture,
        original: &original,
        root: &custody.root,
        package: &package,
    })
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn connect_supervised_fixture(
    tools: crate::release::crate_release_source_view::ProjectedSourceTools,
    view_path: std::path::PathBuf,
    socket: std::os::unix::net::UnixStream,
    boot_deadline: u64,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<
    Option<crate::release::crate_release_source_view::SupervisedSourceExecution>,
    Box<dyn std::error::Error>,
> {
    use std::os::unix::fs::PermissionsExt as _;
    let context: serde_json::Value = serde_json::from_slice(&tools.context(false)?)?;
    let canonical_intent = velnor_actions_contract::canonical_json_bytes(intent)?;
    assert_eq!(
        context["identity"]["intent_sha256"],
        crate::planner::generator::sha256_hex(&canonical_intent),
        "source session and upload permit must bind the same approved intent"
    );
    if std::env::var("VELNOR_TEST_CUSTODY_FAULT").as_deref() == Ok("expose") {
        let view = view_path;
        let moved = view.with_extension("displaced");
        let result = tools.connect_with_exposure_hook(socket, boot_deadline, || {
            fs::rename(&view, &moved).map_err(|_| "fixture failure")?;
            fs::create_dir(&view).map_err(|_| "fixture failure")?;
            fs::set_permissions(&view, fs::Permissions::from_mode(0o711))
                .map_err(|_| "fixture failure")?;
            Ok(())
        });
        fs::remove_dir(&view)?;
        fs::rename(&moved, &view)?;
        assert!(result.is_err(), "replacement during exposure accepted");
        return Ok(None);
    }
    Ok(Some(tools.connect(socket, boot_deadline)?))
}
