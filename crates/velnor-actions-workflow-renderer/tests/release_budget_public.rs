// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[test]
fn budget_capture_binds_job_identity_and_original_start() -> Result<(), Box<dyn std::error::Error>>
{
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let status = std::process::Command::new("python3")
        .arg(root.join("tests/release_budget_fixture.py"))
        .env("BUDGET_SCRIPT", root.join("src/release_budget.py"))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn hosted_containment_requires_expected_result_and_empty_scope()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let status = std::process::Command::new("python3")
        .arg(root.join("tests/release_budget_systemd_logic.py"))
        .arg(root.join("tests/release_budget_systemd_fixture.py"))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn hosted_supervisor_runs_candidate_scenarios_and_controller_regressions()
-> Result<(), Box<dyn std::error::Error>> {
    let release = std::fs::read_to_string("/etc/os-release")?;
    if std::env::consts::OS != "linux" || std::env::consts::ARCH != "x86_64" {
        return Err("hosted supervisor qualification requires Linux x86_64".into());
    }
    assert!(release.lines().any(|line| line == "ID=ubuntu"));
    assert!(release.lines().any(|line| line == "VERSION_ID=\"26.04\""));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    for (file, arguments) in [
        ("release_manager_scenario_controller_test.py", &[][..]),
        ("release_manager_home_access_test.py", &[][..]),
        (
            "release_manager_systemd_test.py",
            &["RealManagerTest.test_reobserved_cgroup_cannot_change_invocation"][..],
        ),
        ("release_manager_qualification.py", &["--execute"][..]),
    ] {
        let status = std::process::Command::new("/usr/bin/sudo")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", "/nonexistent")
            .env("LANG", "C")
            .args([
                "-n",
                "/usr/bin/env",
                "-i",
                "PATH=/usr/bin:/bin",
                "HOME=/nonexistent",
                "LANG=C",
                "/usr/bin/python3",
                "-I",
                "-S",
                "-B",
            ])
            .arg(root.join(file))
            .args(arguments)
            .status()?;
        assert!(
            status.success(),
            "hosted supervisor qualification failed: {file}"
        );
    }
    Ok(())
}

#[test]
fn publisher_step_fits_github_limit_and_binds_generator_pins()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::StepKind;
    use velnor_actions_workflow_renderer::{
        join_argv_for_run, release_root_publisher::publisher_step,
    };
    let source = "a".repeat(40);
    let manifest = "b".repeat(64);
    for bootstrap in [false, true] {
        let step = publisher_step(bootstrap, "0.1.75", &source, &manifest)?;
        let StepKind::Shell { run, env } = step.kind else {
            return Err("publisher step is not shell".into());
        };
        assert!(join_argv_for_run(&run)?.len() <= 21_000);
        assert_eq!(run.last().map(String::as_str), Some(manifest.as_str()));
        assert_eq!(
            run.get(run.len() - 2).map(String::as_str),
            Some(source.as_str())
        );
        assert_eq!(env.contains_key("CARGO_REGISTRY_TOKEN"), bootstrap);
        assert_eq!(
            env.get("VELNOR_RELEASE_BUDGET").map(String::as_str),
            Some("${{ steps.release-budget.outputs.budget }}")
        );
        let rejected = std::process::Command::new(&run[3])
            .args(&run[4..])
            .env_clear()
            .output()?;
        assert!(!rejected.status.success());
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains("publisher first stage rejected")
        );
    }
    assert!(publisher_step(false, "0.1.75", "invalid", &manifest).is_err());
    Ok(())
}
