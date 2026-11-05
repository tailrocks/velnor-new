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
    assert!(release.lines().any(|line| line == "VERSION_ID=\"24.04\""));
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
            .args(["-n", "/usr/bin/python3", "-I", "-S", "-B"])
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
