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
        ("release_source_names_test.py", &[][..]),
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
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("workspace root")?;
    let result = std::process::Command::new("cargo")
        .args([
            "nextest",
            "run",
            "--locked",
            "-p",
            "velnor-actions-cli",
            "--test",
            "crate_release_prepare_pr_public",
            "--run-ignored",
            "only",
            "-E",
            "test(hosted_managed_publisher_export_is_bounded_and_quiescent)",
            "--no-tests",
            "fail",
        ])
        .current_dir(workspace)
        .status()?;
    assert!(
        result.success(),
        "hosted managed-export qualification failed"
    );
    Ok(())
}

#[test]
fn publisher_step_fits_github_limit_and_binds_generator_pins()
-> Result<(), Box<dyn std::error::Error>> {
    use base64::Engine as _;
    use std::io::{Read as _, Write as _};
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
        let command = join_argv_for_run(&run)?;
        assert!(command.len() <= 21_000);
        let mut shell = std::process::Command::new("/bin/bash")
            .arg("-n")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        shell
            .stdin
            .take()
            .ok_or("missing shell stdin")?
            .write_all(command.as_bytes())?;
        assert!(
            shell.wait()?.success(),
            "generated publisher command is invalid shell"
        );
        let encoded = run[6]
            .split_once("base64.b64decode('")
            .ok_or("missing encoded bundle")?
            .1
            .split_once("',validate=True)")
            .ok_or("invalid encoded bundle")?
            .0;
        let compressed = base64::engine::general_purpose::STANDARD.decode(encoded)?;
        let mut decoder = flate2::read::ZlibDecoder::new(compressed.as_slice());
        let mut payload = Vec::new();
        decoder.read_to_end(&mut payload)?;
        let bundle: serde_json::Value = serde_json::from_slice(&payload)?;
        for (key, expected) in [
            (
                "stage",
                include_str!("../src/release_publish_first_stage.py"),
            ),
            ("entry", include_str!("../src/release_publish_entry.py")),
            (
                "bootstrap",
                include_str!("../src/release_generator_bootstrap.sh"),
            ),
            ("manager", include_str!("../src/release_manager_bus.py")),
            (
                "lifecycle",
                include_str!("../src/release_manager_lifecycle.py"),
            ),
            ("helper", include_str!("../src/release_supervisor_exec.py")),
        ] {
            assert_eq!(bundle[key].as_str(), Some(expected));
        }
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

#[test]
fn generated_shell_arguments_preserve_apostrophes() -> Result<(), Box<dyn std::error::Error>> {
    for value in ["'", "''", "a'b", "a'b c", "'leading", "trailing'"] {
        let command = velnor_actions_workflow_renderer::join_argv_for_run(&[
            "/usr/bin/printf".to_owned(),
            "%s".to_owned(),
            value.to_owned(),
        ])?;
        let output = std::process::Command::new("/bin/bash")
            .args(["-c", &command])
            .env_clear()
            .output()?;
        assert!(
            output.status.success(),
            "generated shell failed for {value:?}"
        );
        assert_eq!(output.stdout, value.as_bytes());
    }
    Ok(())
}

#[test]
fn package_names_match_cargo_contract() {
    use velnor_actions_workflow_renderer::release_spec::validate_package_name;
    assert!(validate_package_name(&"a".repeat(64)).is_ok());
    assert!(validate_package_name(&"a".repeat(65)).is_err());
    for (name, valid) in [
        ("_codec", true),
        ("alpha-2", true),
        ("2codec", false),
        ("-codec", false),
        ("", false),
        ("é", false),
    ] {
        assert_eq!(validate_package_name(name).is_ok(), valid, "{name:?}");
    }
}
