// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use velnor_actions_workflow_renderer::release_gates::{
    ReleasePlzAuth, isolated_release_plz_argv, release_executable_env, render_release_plz_argv,
};

#[test]
fn release_tool_runs_only_with_the_verified_toolchain_and_allowed_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::Builder::new()
        .prefix("release custody ; $ ")
        .tempdir()?;
    let root_path = root.path().to_str().ok_or("non-utf8 temp path")?;
    let bin = root
        .path()
        .join("velnor/rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin");
    let hostile = root.path().join("hostile");
    fs::create_dir_all(&bin)?;
    fs::create_dir_all(&hostile)?;
    for (path, script) in [
        (bin.join("cargo"), "#!/bin/sh\nprintf 'verified'\n"),
        (hostile.join("cargo"), "#!/bin/sh\nprintf 'hostile'\n"),
        (
            root.path().join("release-plz"),
            "#!/bin/sh\n/usr/bin/env > \"$RUNNER_TEMP/observed\"\ncargo > \"$RUNNER_TEMP/cargo-result\"\n",
        ),
    ] {
        fs::write(&path, script)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let payload = vec![
        root.path()
            .join("release-plz")
            .to_string_lossy()
            .into_owned(),
    ];
    let argv = isolated_release_plz_argv(payload, ReleasePlzAuth::Trusted);
    let env = release_executable_env("1.97.1", "ubuntu-26.04")?;
    assert!(release_executable_env("1.97.1", "self-hosted").is_err());
    assert!(release_executable_env("../../hostile", "ubuntu-26.04").is_err());
    let mut command = Command::new("/bin/sh");
    command.current_dir(root.path());
    command.arg("-c").arg(render_release_plz_argv(&argv)?);
    command.env("HOME", root_path).env("RUNNER_TEMP", root_path);
    command.env("PATH", format!("{}:/usr/bin:/bin", hostile.display()));
    command.env("RUSTC_WRAPPER", "hostile");
    command.env("RUSTC", "hostile");
    command.env("RUSTDOC", "hostile");
    command.env("RUSTC_WORKSPACE_WRAPPER", "hostile");
    command.env("CARGO_BUILD_RUSTC", "hostile");
    command.env("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER", "hostile");
    command.env("RUSTUP_TOOLCHAIN", "hostile");
    command.env("CARGO_REGISTRY_TOKEN", "hostile");
    command.env("GIT_TOKEN", "synthetic ; $(touch injected)");
    command.env("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "expected-oidc-token");
    command.env("ACTIONS_ID_TOKEN_REQUEST_URL", "expected-oidc-url");
    command.env("MISE_EXEC_AUTO_INSTALL", "true");
    command.env("RUSTUP_AUTO_INSTALL", "0");
    for (key, value) in env {
        command.env(key, value.replace("${{ runner.temp }}", root_path));
    }
    assert!(command.status()?.success());
    assert_eq!(
        fs::read_to_string(root.path().join("cargo-result"))?,
        "verified"
    );
    let observed = fs::read_to_string(root.path().join("observed"))?;
    for denied in [
        "RUSTC_WRAPPER=",
        "RUSTC_WORKSPACE_WRAPPER=",
        "CARGO_BUILD_RUSTC=",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=",
        "RUSTUP_TOOLCHAIN=",
        "CARGO_REGISTRY_TOKEN=",
        "MISE_EXEC_AUTO_INSTALL=",
    ] {
        assert!(!observed.contains(denied), "unexpected {denied}");
    }
    assert!(observed.contains(&format!("RUSTC={}/rustc", bin.display())));
    assert!(observed.contains(&format!("RUSTDOC={}/rustdoc", bin.display())));
    assert!(observed.contains(&format!("CARGO={}/cargo", bin.display())));
    assert!(observed.contains("RUSTUP_AUTO_INSTALL=0"));
    assert!(observed.contains("GIT_TOKEN=synthetic ; $(touch injected)"));
    assert!(!root.path().join("injected").exists());
    assert!(observed.contains("ACTIONS_ID_TOKEN_REQUEST_TOKEN=expected-oidc-token"));
    assert!(observed.contains("ACTIONS_ID_TOKEN_REQUEST_URL=expected-oidc-url"));
    Ok(())
}
