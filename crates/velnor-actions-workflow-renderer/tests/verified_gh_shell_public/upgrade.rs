// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn rendered_upgrade_publish_script() -> Result<String, Box<dyn std::error::Error>> {
    rendered_upgrade_publish_script_for("ubuntu-26.04")
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
    rendered_upgrade_publish_script_for("ubuntu-26.04")?;
    for runner in [
        "ubuntu-22.04",
        "ubuntu-24.04",
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

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
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
