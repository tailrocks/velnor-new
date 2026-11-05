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
