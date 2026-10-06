//! Ambient wrapper authority must never survive a child policy.

use std::ffi::OsString;

use velnor_actions_mise::command::EnvPolicy;

#[test]
fn ambient_owned_wrapper_authority_strips_for_every_policy() {
    let keys = [
        "MISE_OWNED_CARGO_WRAPPER",
        "MISE_OWNED_CARGO_WRAPPER_SHA256",
        "MISE_OWNED_CARGO_WRAPPER_FUTURE_AUTHORITY",
    ];
    let parent: Vec<_> = keys
        .iter()
        .map(|key| (OsString::from(*key), OsString::from("caller-controlled")))
        .collect();
    for policy in [
        EnvPolicy::Bootstrap,
        EnvPolicy::Baseline,
        EnvPolicy::Verify,
        EnvPolicy::Discovery,
        EnvPolicy::RepoTask,
    ] {
        let env = policy.child_env(&parent, &[]);
        for key in keys {
            assert!(
                !env.iter().any(|(name, _)| name == key),
                "{policy:?}: ambient {key} survived"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn live_ambient_wrapper_authority_strips_for_every_policy() -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("velnor-wrapper-env-{}", std::process::id()));
    std::fs::create_dir(&root).map_err(|err| err.to_string())?;
    for program in ["mise", "git"] {
        let path = root.join(program);
        std::fs::write(
            &path,
            "#!/bin/sh\necho VELNOR_WRAPPER_FAKE_TOOL\nexec /usr/bin/env\n",
        )
        .map_err(|err| err.to_string())?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|err| err.to_string())?;
    }
    let output =
        std::process::Command::new(std::env::current_exe().map_err(|err| err.to_string())?)
            .args([
                "--exact",
                "impl_mise_wrapper_authority::ambient_wrapper_authority_child",
                "--nocapture",
            ])
            .env("VELNOR_WRAPPER_ENV_CHILD", "1")
            .env("PATH", &root)
            .env("MISE_OWNED_CARGO_WRAPPER", "/hostile/mbx")
            .env("MISE_OWNED_CARGO_WRAPPER_SHA256", "a".repeat(64))
            .env("MISE_OWNED_CARGO_WRAPPER_FUTURE_AUTHORITY", "hostile")
            .output()
            .map_err(|err| err.to_string());
    std::fs::remove_dir_all(&root).map_err(|err| err.to_string())?;
    let output = output?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "child failed: {output:?}");
    assert!(
        stdout.contains("VELNOR_WRAPPER_ENV_CHILD_PROVEN"),
        "child did not execute: {stdout}"
    );
    assert!(
        stdout.contains("1 passed"),
        "child test not selected: {stdout}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn ambient_wrapper_authority_child() -> Result<(), String> {
    use velnor_actions_mise::{
        GitRequest, IsolatedCommand, PinnedTool, PinnedToolExec, ToolCatalog,
    };
    if std::env::var_os("VELNOR_WRAPPER_ENV_CHILD").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return Ok(());
    }
    for key in [
        "MISE_OWNED_CARGO_WRAPPER",
        "MISE_OWNED_CARGO_WRAPPER_SHA256",
        "MISE_OWNED_CARGO_WRAPPER_FUTURE_AUTHORITY",
    ] {
        assert!(
            std::env::var_os(key).is_some(),
            "missing hostile parent {key}"
        );
    }
    let specs = ["rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned()];
    let gh = PinnedToolExec::new(vec![PinnedTool::Gh], std::ffi::OsStr::new("gh"), Vec::new())
        .map_err(|err| err.to_string())?;
    let commands = [
        IsolatedCommand::mise_install(&specs).map_err(|err| err.to_string())?,
        gh.command(&ToolCatalog::pinned())
            .map_err(|err| err.to_string())?,
        IsolatedCommand::mise_exec(&specs, &[OsString::from("cargo")])
            .map_err(|err| err.to_string())?,
        GitRequest::rev_parse(Vec::new()).command(),
        IsolatedCommand::repo_task("/usr/bin/env", Vec::new(), &[])
            .map_err(|err| err.to_string())?,
    ];
    for (index, command) in commands.into_iter().enumerate() {
        let output = command.run().map_err(|err| err.to_string())?;
        assert!(output.success, "policy {index}: {output:?}");
        let stdout = output.stdout_text("env").map_err(|err| err.to_string())?;
        if index < 4 {
            assert!(
                stdout.contains("VELNOR_WRAPPER_FAKE_TOOL"),
                "fake tool did not execute: {stdout}"
            );
        }
        assert!(
            !stdout
                .lines()
                .any(|line| line.starts_with("MISE_OWNED_CARGO_WRAPPER")),
            "policy {index}: ambient authority survived: {stdout}"
        );
    }
    println!("VELNOR_WRAPPER_ENV_CHILD_PROVEN");
    Ok(())
}
