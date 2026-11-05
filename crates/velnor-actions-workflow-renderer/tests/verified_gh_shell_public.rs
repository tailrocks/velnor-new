// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_workflow_renderer::verified_gh_shell::command_for;

fn prefix() -> Vec<String> {
    let pin = velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64;
    [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        &format!("gh@{}", pin.version),
        "--",
        "gh",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn procfs_config_custody_rejects_same_user_poisoning() -> Result<(), Box<dyn std::error::Error>> {
    let pin = velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64;
    let source = std::env::var_os("MISE_DATA_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|v| std::path::PathBuf::from(v).join(".local/share/mise"))
        })
        .ok_or("mise data")?;
    let relative = format!(
        "installs/gh/{0}/gh_{0}_{1}/bin/gh",
        pin.version, pin.platform
    );
    let shell = velnor_actions_workflow_renderer::publish_scripts::gh_function(&prefix())?;
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/verified_gh_procfs_cases.py");
    let result = std::process::Command::new("/usr/bin/python3")
        .arg(fixture)
        .arg(shell)
        .arg(source)
        .arg(relative)
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}

#[test]
fn rejects_unqualified_mise_arguments() {
    let mut argv = prefix();
    assert!(command_for(&argv).is_ok());
    argv[5] = "gh@0.0.0".into();
    assert!(command_for(&argv).is_err());
    argv[5] = format!(
        "gh@{}",
        velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64.version
    );
    argv.push("--extra".into());
    assert!(command_for(&argv).is_err());
    let mut dynamic = prefix();
    dynamic[5] = "gh@$VELNOR_RELEASE_GH_VERSION".into();
    assert!(command_for(&dynamic).is_ok());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn rendered_shell_runs_only_the_verified_github_cli() -> Result<(), Box<dyn std::error::Error>> {
    use std::{os::unix::fs::PermissionsExt as _, path::PathBuf, process::Command};
    let pin = velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64;
    let source_root = std::env::var_os("MISE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(|v| PathBuf::from(v).join("mise")))
        .or_else(|| std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".local/share/mise")))
        .ok_or("mise data")?;
    let relative = format!(
        "installs/gh/{0}/gh_{0}_{1}/bin/gh",
        pin.version, pin.platform
    );
    let source = source_root.join(&relative);
    let root = tempfile::tempdir()?;
    let candidate = root.path().join(&relative);
    std::fs::create_dir_all(candidate.parent().ok_or("parent")?)?;
    std::fs::copy(&source, &candidate)?;
    let shell = velnor_actions_workflow_renderer::publish_scripts::gh_function(&prefix())?;
    let run = |script: &str| -> Result<std::process::Output, Box<dyn std::error::Error>> {
        Ok(Command::new("/bin/sh")
            .args(["-c", script])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root.path())
            .env("MISE_DATA_DIR", root.path())
            .env("TMPDIR", root.path())
            .env("GH_TOKEN", "offline-fixture")
            .output()?)
    };
    let original = run(&format!("{shell}\ngh --version"))?;
    assert!(
        original.status.success(),
        "{}",
        String::from_utf8_lossy(&original.stderr)
    );
    assert!(
        String::from_utf8_lossy(&original.stdout).contains(&format!("gh version {}", pin.version))
    );
    let mut dynamic = prefix();
    dynamic[5] = "gh@$VELNOR_RELEASE_GH_VERSION".into();
    let dynamic_shell = velnor_actions_workflow_renderer::publish_scripts::gh_function(&dynamic)?;
    assert!(
        !run(&format!("{dynamic_shell}\ngh --version"))?
            .status
            .success()
    );
    assert!(
        run(&format!(
            "{dynamic_shell}\nexport VELNOR_RELEASE_GH_VERSION={}; gh --version",
            pin.version
        ))?
        .status
        .success()
    );
    let direct = |args: &[&str]| -> Result<std::process::Output, Box<dyn std::error::Error>> {
        Ok(Command::new(&source)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root.path())
            .env("TMPDIR", root.path())
            .env("GH_TOKEN", "offline-fixture")
            .output()?)
    };
    assert!(
        direct(&[
            "alias",
            "set",
            "probe",
            "!test -n \"$GH_TOKEN\" && touch \"$TMPDIR/executed\""
        ])?
        .status
        .success()
    );
    assert!(direct(&["probe"])?.status.success());
    assert!(root.path().join("executed").exists());
    std::fs::remove_file(root.path().join("executed"))?;
    assert!(!run(&format!("{shell}\ngh probe"))?.status.success());
    assert!(!root.path().join("executed").exists());
    std::fs::write(
        &candidate,
        b"#!/bin/sh\ntest -n \"$GH_TOKEN\" && touch \"$TMPDIR/executed\"\necho gh version 2.103.3\n",
    )?;
    std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o755))?;
    let fake = run(&format!("{shell}\ngh --version"))?;
    assert!(!fake.status.success());
    assert!(!root.path().join("executed").exists());
    Ok(())
}

#[test]
fn qualified_archive_rejects_ambiguous_or_escaping_members()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("/usr/bin/python3")
        .env_clear()
        .arg(root.join("tests/verified_gh_acquire_cases.py"))
        .arg(root.join("src/verified_gh_acquire.py"))
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn bootstrap_gives_its_acquisition_process_only_safe_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let bootstrap = velnor_actions_workflow_renderer::verified_gh_shell::bootstrap();
    let start = bootstrap
        .find("exec(bytes.fromhex(\"")
        .ok_or("missing program")?
        + "exec(bytes.fromhex(\"".len();
    let end = start
        + bootstrap[start..]
            .find("\"))")
            .ok_or("missing program end")?;
    let test_program = "import os\nassert set(os.environ) <= {'RUNNER_TEMP', 'PATH', 'LC_CTYPE', 'CPATH', 'LIBRARY_PATH', 'MANPATH', 'SDKROOT', '__CF_USER_TEXT_ENCODING'}\nassert not any(k in os.environ for k in ('GH_TOKEN', 'GIT_TOKEN', 'CARGO_REGISTRY_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'TMPDIR'))\nprint(os.environ['RUNNER_TEMP'])\n";
    let mut encoded = String::new();
    for byte in test_program.bytes() {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    let script = format!("{}{}{}", &bootstrap[..start], encoded, &bootstrap[end..]);
    let output = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .env_clear()
        .env("RUNNER_TEMP", temp.path())
        .env("TMPDIR", temp.path())
        .env("GH_TOKEN", "fixture")
        .env("GIT_TOKEN", "fixture")
        .env("CARGO_REGISTRY_TOKEN", "fixture")
        .env("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "fixture")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn verified_helper_authenticates_real_git_without_untrusted_helpers()
-> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;
    let data = std::env::var_os("MISE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(|v| PathBuf::from(v).join("mise")))
        .or_else(|| std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".local/share/mise")))
        .ok_or("mise data")?;
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/verified_gh_git_cases.py");
    let token_dir = tempfile::tempdir()?;
    let fixture_token = format!(
        "velnor-fixture-{}",
        token_dir
            .path()
            .file_name()
            .ok_or("fixture token")?
            .to_string_lossy()
    );
    let status = std::process::Command::new("/usr/bin/python3")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("MISE_DATA_DIR", data)
        .env("GH_TOKEN", fixture_token)
        .env("VELNOR_PUBLISH", rendered_upgrade_publish_script()?)
        .env(
            "VELNOR_TRANSPORT_SOURCE",
            include_str!("../src/verified_git_transport.py"),
        )
        .arg(script)
        .status()?;
    assert!(status.success(), "local Git authentication fixture failed");
    Ok(())
}

fn rendered_upgrade_publish_script() -> Result<String, Box<dyn std::error::Error>> {
    rendered_upgrade_publish_script_for("ubuntu-24.04")
}

fn rendered_upgrade_publish_script_for(runner: &str) -> Result<String, Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::upgrades::{UpgradesSpec, render_upgrades_workflow};
    let config = velnor_actions_contract::config::UpgradesConfig {
        enabled: true,
        ..Default::default()
    };
    let mise = velnor_actions_workflow_renderer::MiseSetup {
        uses: format!(
            "jdx/mise-action@{}",
            velnor_actions_actionlint::actions::MISE_ACTION_SHA
        ),
        version: velnor_actions_mise::MISE_VERSION.to_owned(),
        sha256: velnor_actions_workflow_renderer::setup::MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    };
    let spec = UpgradesSpec {
        config,
        runs_on: runner.into(),
        checkout_uses: velnor_actions_workflow_renderer::steps::CHECKOUT_USES.into(),
        mise_setup: mise,
        gh_version: velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64
            .version
            .into(),
        generator_version: "0.1.73".into(),
        generate_tools: vec!["jactionlint@1.0.0".into()],
        verify_rust_script: "${VELNOR_TOOL_RUST}".into(),
        jactionlint_verify:
            velnor_actions_contract::config::jactionlint_verifier::jactionlint_verifier_script(
                "1.0.0",
                velnor_actions_workflow_renderer::VERIFY_PATH,
            ),
    };
    let rendered = render_upgrades_workflow(&spec)?;
    let workflow: serde_json::Value = serde_saphyr::from_str(&rendered.bytes)?;
    let steps = workflow["jobs"]["publish"]["steps"]
        .as_array()
        .ok_or("publish steps")?;
    let step = steps
        .iter()
        .find(|step| step["name"] == "Push the upgrade branch and open or update its pull request")
        .ok_or("publish step")?;
    for name in [
        "BASH_ENV",
        "BASHOPTS",
        "SHELLOPTS",
        "ENV",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
        "LD_DEBUG",
        "LD_PROFILE",
        "GCONV_PATH",
        "LOCPATH",
    ] {
        assert_eq!(step["env"][name].as_str(), Some(""));
    }
    Ok(step["run"].as_str().ok_or("publish script")?.to_owned())
}

#[test]
fn upgrades_reject_unqualified_runner_platforms() -> Result<(), Box<dyn std::error::Error>> {
    for runner in ["ubuntu-22.04", "ubuntu-24.04", "ubuntu-26.04"] {
        rendered_upgrade_publish_script_for(runner)?;
    }
    for runner in [
        "ubuntu-22.04-arm",
        "ubuntu-24.04-arm",
        "ubuntu-26.04-arm",
        "macos-15",
        "windows-2025",
        "ubuntu-custom",
    ] {
        let error =
            rendered_upgrade_publish_script_for(runner).expect_err("runner must be rejected");
        assert!(
            error
                .to_string()
                .contains("upgrades_requires_github_hosted_linux_amd64")
        );
    }
    Ok(())
}

#[test]
fn upgrades_publish_uses_an_isolated_transport() -> Result<(), Box<dyn std::error::Error>> {
    let script = rendered_upgrade_publish_script()?;
    assert!(script.contains("remote_git()"));
    assert!(script.contains("GIT_ALLOW_PROTOCOL=https"));
    assert!(!script.contains("auth setup-git"));
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn verified_shell_checks_real_signed_attestation_without_writable_home()
-> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;
    let data = std::env::var_os("MISE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(|v| PathBuf::from(v).join("mise")))
        .or_else(|| std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".local/share/mise")))
        .ok_or("mise data")?;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../velnor-actions-orchestrator/tests/attestation_fixtures/historical");
    let shell = velnor_actions_workflow_renderer::publish_scripts::gh_function(&prefix())?;
    let script = format!(
        "{shell}\ngh attestation verify \"$1\" --bundle \"$2\" --custom-trusted-root \"$3\" --repo tailrocks/velnor-new --format json >/dev/null"
    );
    let output = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg("gh-attestation")
        .arg(root.join("release-manifest.json"))
        .arg(root.join("release-manifest.json.intoto.jsonl"))
        .arg(root.join("trusted-root.jsonl"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("MISE_DATA_DIR", data)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn upgrade_git_init_receives_no_credentials() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt as _;
    let script = rendered_upgrade_publish_script()?;
    assert!(script.contains("local_git init --bare"));
    let function = script
        .lines()
        .find(|line| line.starts_with("local_git()"))
        .ok_or("local git function")?;
    let temp = tempfile::tempdir()?;
    let fake = temp.path().join("git");
    std::fs::write(&fake, b"#!/bin/sh\ntest -z \"${GH_TOKEN+x}${GITHUB_TOKEN+x}${CARGO_REGISTRY_TOKEN+x}${ACTIONS_ID_TOKEN_REQUEST_TOKEN+x}\"\n")?;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))?;
    let replacement = fake.to_str().ok_or("fixture path")?;
    let body = format!(
        "{}\nlocal_git init --bare",
        function.replace("/usr/bin/git", replacement)
    );
    let output = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(body)
        .env("GH_TOKEN", "fixture")
        .env("GITHUB_TOKEN", "fixture")
        .env("CARGO_REGISTRY_TOKEN", "fixture")
        .env("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "fixture")
        .output()?;
    assert!(output.status.success(), "local Git inherited credentials");
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn upgrade_git_init_ignores_hostile_loader() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let library = temp.path().join("adversary.so");
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/verified_gh_preload.c");
    let compiled = std::process::Command::new("/usr/bin/cc")
        .args(["-shared", "-fPIC", "-o"])
        .arg(&library)
        .arg(source)
        .status()?;
    assert!(compiled.success());
    let marker = temp.path().join("executed");
    let run = |body: &str| -> Result<std::process::Output, Box<dyn std::error::Error>> {
        Ok(std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(body)
            .arg("fixture")
            .arg(&library)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("GH_TOKEN", "offline-fixture")
            .env("MARKER", &marker)
            .output()?)
    };
    assert!(
        run("export LD_PRELOAD=\"$1\"; /usr/bin/true")?
            .status
            .success()
    );
    assert!(marker.exists(), "preload adversary did not execute");
    std::fs::remove_file(&marker)?;
    let script = rendered_upgrade_publish_script()?;
    let function = script
        .lines()
        .find(|line| line.starts_with("local_git()"))
        .ok_or("local git function")?;
    let body =
        format!("export LD_PRELOAD=\"$1\"; {function}\nlocal_git init --bare --quiet \"$2\"");
    let output = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(body)
        .arg("fixture")
        .arg(&library)
        .arg(temp.path().join("repo"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GH_TOKEN", "offline-fixture")
        .env("MARKER", &marker)
        .output()?;
    assert!(output.status.success(), "isolated Git init failed");
    assert!(
        !marker.exists(),
        "loader adversary reached credentialed Git init"
    );
    Ok(())
}
