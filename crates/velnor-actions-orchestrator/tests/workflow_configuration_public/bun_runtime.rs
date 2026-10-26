// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use super::workflow_configuration_fixture;
use velnor_actions_contract::StepKind;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::publish_render::PUBLISH_WORKFLOW_PATH;

#[test]
fn prepared_bun_workflow_forces_bun_for_scripts_and_keeps_frozen_install()
-> Result<(), Box<dyn Error>> {
    let repo = make_bun_repo()?;
    write_bun_fixture(repo.path())?;
    let prepared = prepare(repo.path())?;
    let rendered = render_staged_tree(&prepared)?;
    assert!(!rendered.is_empty(), "rendered workflow tree is empty");
    let publish_workflow = rendered
        .get(PUBLISH_WORKFLOW_PATH)
        .ok_or("rendered npm publication workflow is missing")?;

    assert_npm_pack_forces_bun(publish_workflow)?;
    assert_project_scripts_force_bun(&prepared);
    assert_frozen_install_stays_unchanged(&prepared);
    Ok(())
}

fn make_bun_repo() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let config = include_str!(
        "../../../../fixtures/bun-workspace-suites/input/.velnor/config.toml"
    )
    .replace("roots = [\".\"]", "roots = [\"frontend\"]")
    .replace(
        "default_branch = \"main\"",
        "default_branch = \"main\"\npolicy = \"consumer-v1\"",
    )
    .replace(
        "scripts = [\"typecheck\", \"lint\", \"test\", \"test:browser\"]",
        "scripts = [\"typecheck\", \"lint\", \"test\", \"test:browser\", \"pack:portable\"]",
    ) + "\n[[publish.npm]]\nid = \"fixture\"\nroot = \"frontend\"\nmember = \"@fixture/web\"\npack_script = \"pack:portable\"\ntarballs = \"frontend/.portable/tarballs\"\nregistry = \"github\"\n";
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    workflow_configuration_fixture::git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/fixture/repo.git",
        ],
        repo.path(),
    )?;
    Ok(repo)
}

fn assert_project_scripts_force_bun(prepared: &velnor_actions_orchestrator::GenerationPreparation) {
    let member_commands = shell_commands_for(prepared, "Bun / @fixture/web");
    let unit_commands = shell_commands_for(prepared, "Bun / @fixture/web / unit");
    let browser_commands = shell_commands_for(prepared, "Bun / @fixture/web / browser");
    let report_commands = shell_commands_for(prepared, "Bun / @fixture/web / browser / report");
    let scripts = member_commands
        .iter()
        .chain(&unit_commands)
        .chain(&browser_commands)
        .chain(&report_commands)
        .collect::<Vec<_>>();

    for (commands, script) in [
        (&member_commands, "typecheck"),
        (&member_commands, "lint"),
        (&member_commands, "pack:portable"),
        (&unit_commands, "test"),
        (&browser_commands, "test:browser"),
    ] {
        assert!(
            commands.iter().any(|command| {
                command.contains(&format!(
                    "'bun' '--bun' 'run' '--cwd=frontend/apps/web' '{script}'"
                ))
            }),
            "missing Bun-forced script {script}: {commands:?}"
        );
    }
    assert!(report_commands.iter().any(|command| {
        command.contains("'bun' '--bun' 'run'") && command.contains("'playwright' 'merge-reports'")
    }));
    assert!(
        scripts
            .iter()
            .all(|command| !command.contains("'bun' 'run'"))
    );
    assert!(scripts.iter().all(|command| !command.contains("bunx")));
    assert!(scripts.iter().all(|command| !command.contains("npx")));
}

fn assert_npm_pack_forces_bun(publish_workflow: &str) -> Result<(), Box<dyn Error>> {
    let pack_section = publish_workflow
        .split("name: Pack npm tarball")
        .nth(1)
        .and_then(|section| section.split("name: Upload tarball").next())
        .ok_or("npm publication pack step is missing")?;
    let bun_flag = pack_section.find("--bun");
    let run_command = pack_section.find("'run'");
    assert!(
        matches!((bun_flag, run_command), (Some(flag), Some(run)) if flag < run)
            && pack_section.contains("pack:portable"),
        "npm publication pack command does not force Bun: {pack_section}"
    );
    Ok(())
}

fn assert_frozen_install_stays_unchanged(
    prepared: &velnor_actions_orchestrator::GenerationPreparation,
) {
    let install_steps = prepared
        .workflow
        .ir
        .jobs
        .values()
        .flat_map(|job| {
            job.steps
                .iter()
                .filter(|step| step.name == "Install dependencies")
                .map(move |step| (job.display_name.as_str(), step))
        })
        .collect::<Vec<_>>();
    assert!(!install_steps.is_empty(), "Bun install steps are missing");
    assert_eq!(
        install_steps.len(),
        5,
        "unexpected number of Bun install steps: {install_steps:?}"
    );
    for expected_job in [
        "Bun / @fixture/ui",
        "Bun / @fixture/web",
        "Bun / @fixture/web / unit",
        "Bun / @fixture/web / browser",
        "Bun / @fixture/web / browser / report",
    ] {
        let job_steps = install_steps
            .iter()
            .filter(|(display_name, _)| *display_name == expected_job)
            .collect::<Vec<_>>();
        assert_eq!(
            job_steps.len(),
            1,
            "expected one Bun install step in {expected_job}: {job_steps:?}"
        );
        let (_, step) = *job_steps[0];
        assert!(
            matches!(&step.kind, StepKind::Shell { run, .. } if run.len() == 3),
            "Bun install step in {expected_job} must be a three-argument shell command: {step:?}"
        );
        if let StepKind::Shell { run, .. } = &step.kind
            && run.len() == 3
        {
            let command = &run[2];
            let install = command.find("'bun' 'install'");
            let frozen = command.find("'--frozen-lockfile'");
            assert!(
                matches!((install, frozen), (Some(install), Some(frozen)) if frozen > install),
                "Bun install in {expected_job} must use --frozen-lockfile: {command}"
            );
            assert!(
                !command.contains("'bun' '--bun' 'install'"),
                "Bun install in {expected_job} must not use --bun: {command}"
            );
        }
    }
}

fn shell_commands_for(
    prepared: &velnor_actions_orchestrator::GenerationPreparation,
    display_name: &str,
) -> Vec<String> {
    prepared
        .workflow
        .ir
        .jobs
        .values()
        .filter(|job| job.display_name == display_name)
        .flat_map(|job| &job.steps)
        .filter_map(|step| match &step.kind {
            StepKind::Shell { run, .. } if run.len() == 3 => Some(run[2].clone()),
            _ => None,
        })
        .collect()
}

fn write_bun_fixture(root: &Path) -> Result<(), Box<dyn Error>> {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bun-workspace-suites/input");
    for (source, destination) in [
        ("package.json", "frontend/package.json"),
        ("bun.lock", "frontend/bun.lock"),
        ("mise.toml", "mise.toml"),
        ("mise.toml", "frontend/mise.toml"),
        (".velnor/playwright.lock", ".velnor/playwright.lock"),
        ("apps/web/package.json", "frontend/apps/web/package.json"),
        (
            "packages/ui/package.json",
            "frontend/packages/ui/package.json",
        ),
    ] {
        copy_fixture_file(&fixture.join(source), &root.join(destination))?;
    }

    let root_manifest = root.join("frontend/package.json");
    let mut root_package: serde_json::Value = serde_json::from_slice(&fs::read(&root_manifest)?)?;
    root_package["scripts"]["pack:portable"] = serde_json::json!("bun pm pack");
    fs::write(&root_manifest, serde_json::to_vec(&root_package)?)?;

    let web_manifest = root.join("frontend/apps/web/package.json");
    let mut package: serde_json::Value = serde_json::from_slice(&fs::read(&web_manifest)?)?;
    package["version"] = serde_json::json!("1.2.3");
    package["scripts"]["pack:portable"] = serde_json::json!("bun pm pack");
    package["publishConfig"]["registry"] = serde_json::json!("https://npm.pkg.github.com");
    fs::write(web_manifest, serde_json::to_vec(&package)?)?;
    Ok(())
}

fn copy_fixture_file(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    let parent = destination.parent().ok_or_else(|| {
        format!(
            "fixture destination has no parent: {}",
            destination.display()
        )
    })?;
    fs::create_dir_all(parent)?;
    fs::copy(source, destination)?;
    Ok(())
}
