// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CratePublishCredential, IsolatedCommand};
use std::{ffi::OsString, path::Path, time::Duration};

fn fixture_git()
-> Result<(tempfile::TempDir, crate::release_git_pin::VerifiedGit), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    std::fs::write(root.join("git"), b"fixture")?;
    Ok((temp, crate::release_git_pin::fixture_git(&root)))
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "The complete child isolation fixture is reviewed together"
)]
fn crate_update_exposes_no_credentials_or_write_handles() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::PermissionsExt;
    if std::env::var_os("VELNOR_TEST_UPDATE_CHILD").is_none() {
        return run_update_with_poisoned_environment();
    }
    let (_git_dir, git) = fixture_git()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let home = root.path().join("home");
    std::fs::create_dir(&source)?;
    std::fs::create_dir(&home)?;
    let config = source.join("release-plz.toml");
    let manifest = source.join("Cargo.toml");
    std::fs::write(&config, "")?;
    std::fs::write(&manifest, "")?;
    let program = root.path().join("release-plz");
    std::fs::write(
        &program,
        "#!/bin/sh\ntest \"$1\" = update && test -z \"${GIT_TOKEN+x}${GITHUB_TOKEN+x}${GH_TOKEN+x}${CARGO_REGISTRY_TOKEN+x}${ACTIONS_ID_TOKEN_REQUEST_TOKEN+x}${GITHUB_OUTPUT+x}${HTTP_PROXY+x}\"\n",
    )?;
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
    let env: Vec<(OsString, OsString)> = [
        ("PATH", "/usr/bin:/bin"),
        ("CARGO", "/usr/bin/true"),
        ("RUSTC", "/usr/bin/true"),
        ("RUSTDOC", "/usr/bin/true"),
        ("HOME", home.to_str().ok_or("home")?),
        ("CARGO_HOME", home.to_str().ok_or("home")?),
        ("CARGO_TARGET_DIR", home.to_str().ok_or("home")?),
        ("RUSTUP_HOME", home.to_str().ok_or("home")?),
        ("RUSTUP_AUTO_INSTALL", "0"),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect();
    let args = vec![
        "update".into(),
        "--config".into(),
        config.into_os_string(),
        "--manifest-path".into(),
        manifest.into_os_string(),
    ];
    let command = IsolatedCommand::crate_update(&program, args.clone(), &source, &env, &git)?;
    let parent = [
        "GIT_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "CARGO_REGISTRY_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "GITHUB_OUTPUT",
        "HTTP_PROXY",
    ]
    .into_iter()
    .map(|key| (OsString::from(key), OsString::from("fixture")))
    .collect::<Vec<_>>();
    assert!(
        !command
            .spawn_env(&parent)
            .iter()
            .any(|(key, _)| parent.iter().any(|(forbidden, _)| key == forbidden))
    );
    let child_env = command.spawn_env(&parent);
    assert!(
        child_env
            .iter()
            .any(|(key, value)| key == "GIT_EXEC_PATH" && value == git.exec_path().as_os_str())
    );
    let path = child_env
        .iter()
        .find(|(key, _)| key == "PATH")
        .ok_or("path")?;
    assert_eq!(
        std::env::split_paths(&path.1).next().as_deref(),
        Some(git.exec_path())
    );
    assert!(command.run_bounded(1024, Duration::from_secs(5))?.success);
    let mut release_pr = args.clone();
    release_pr[0] = "release-pr".into();
    assert!(IsolatedCommand::crate_update(&program, release_pr, &source, &env, &git).is_err());
    let mut dirty = args.clone();
    dirty.push("--allow-dirty".into());
    assert!(IsolatedCommand::crate_update(&program, dirty, &source, &env, &git).is_err());
    let mut source_target = env.clone();
    source_target
        .iter_mut()
        .find(|(key, _)| key == "CARGO_TARGET_DIR")
        .ok_or("target")?
        .1 = source.as_os_str().to_owned();
    assert!(
        IsolatedCommand::crate_update(&program, args.clone(), &source, &source_target, &git)
            .is_err()
    );
    let mut injected = env.clone();
    injected.push(("GIT_EXEC_PATH".into(), "/usr/bin".into()));
    assert!(
        IsolatedCommand::crate_update(&program, args.clone(), &source, &injected, &git).is_err()
    );
    let mut hostile = env;
    hostile.push(("GIT_TOKEN".into(), "fixture".into()));
    assert!(IsolatedCommand::crate_update(&program, args, &source, &hostile, &git).is_err());
    Ok(())
}

#[cfg(unix)]
fn run_update_with_poisoned_environment() -> Result<(), Box<dyn std::error::Error>> {
    let mut child = std::process::Command::new(std::env::current_exe()?);
    child.args([
        "--exact",
        "crate_update_exposes_no_credentials_or_write_handles",
    ]);
    child.env("VELNOR_TEST_UPDATE_CHILD", "1");
    for key in [
        "GIT_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "CARGO_REGISTRY_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "GITHUB_OUTPUT",
        "HTTP_PROXY",
    ] {
        child.env(key, "fixture");
    }
    assert!(child.status()?.success());
    Ok(())
}

fn tool_env() -> Vec<(OsString, OsString)> {
    [
        ("PATH", "/usr/bin:/bin"),
        ("CARGO", "/bin/true"),
        ("RUSTC", "/bin/true"),
        ("RUSTDOC", "/bin/true"),
        ("HOME", "/tmp"),
        ("CARGO_HOME", "/tmp"),
        ("RUSTUP_HOME", "/tmp"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect()
}

#[test]
fn crate_publish_exposes_only_the_selected_credentials() -> Result<(), Box<dyn std::error::Error>> {
    let (_git_dir, git) = fixture_git()?;
    let source_dir = tempfile::tempdir()?;
    let source = source_dir.path().canonicalize()?;
    let script = "set -e; test -n \"$GIT_TOKEN\"; test \"$GITHUB_ACTIONS\" = true; test -n \"$ACTIONS_ID_TOKEN_REQUEST_URL\"; test -n \"$ACTIONS_ID_TOKEN_REQUEST_TOKEN\"; test -z \"${CARGO_REGISTRY_TOKEN+x}\"; test -z \"${HTTP_PROXY+x}\"; test -z \"${HOSTILE_TOKEN+x}\"";
    let command = IsolatedCommand::crate_publish(
        Path::new("/bin/sh"),
        vec!["-c".into(), script.into()],
        &source,
        &tool_env(),
        "fixture_git_987",
        CratePublishCredential::Trusted {
            request_url: "https://pipelines.actions.githubusercontent.com/id",
            request_token: "fixture_oidc_789",
        },
        &git,
    )?;
    let debug = format!("{command:?}");
    assert!(!debug.contains("fixture_git_987") && !debug.contains("fixture_oidc_789"));
    assert!(command.run_bounded(1024, Duration::from_secs(5))?.success);
    let parent = vec![
        ("HOSTILE_TOKEN".into(), "unwanted".into()),
        ("HTTP_PROXY".into(), "http://untrusted".into()),
    ];
    let env = command.spawn_env(&parent);
    assert!(
        env.iter()
            .any(|(key, value)| key == "GIT_EXEC_PATH" && value == git.exec_path().as_os_str())
    );
    let path = env.iter().find(|(key, _)| key == "PATH").ok_or("path")?;
    assert_eq!(
        std::env::split_paths(&path.1).next().as_deref(),
        Some(git.exec_path())
    );
    assert!(!env.iter().any(|(key, _)| key == "HOSTILE_TOKEN"
        || key == "HTTP_PROXY"
        || key == "CARGO_REGISTRY_TOKEN"));
    let bootstrap = IsolatedCommand::crate_publish(Path::new("/bin/sh"), vec!["-c".into(), "set -e; test -n \"$GIT_TOKEN\"; test -n \"$CARGO_REGISTRY_TOKEN\"; test -z \"${GITHUB_ACTIONS+x}\"; test -z \"${ACTIONS_ID_TOKEN_REQUEST_TOKEN+x}\"".into()], &source, &tool_env(), "fixture_git_987", CratePublishCredential::Bootstrap { cargo_token: "fixture_cargo_654" }, &git)?;
    assert!(bootstrap.run_bounded(1024, Duration::from_secs(5))?.success);
    assert!(!format!("{bootstrap:?}").contains("fixture_cargo_654"));
    Ok(())
}

#[test]
fn crate_publish_rejects_git_tools_inside_source() -> Result<(), Box<dyn std::error::Error>> {
    let (_git_dir, git) = fixture_git()?;
    assert!(
        IsolatedCommand::crate_publish(
            Path::new("/bin/sh"),
            vec!["release".into()],
            git.exec_path(),
            &tool_env(),
            "fixture_git_987",
            CratePublishCredential::Bootstrap {
                cargo_token: "fixture_cargo_654"
            },
            &git,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn crate_publish_rejects_untrusted_endpoints_and_secret_arguments()
-> Result<(), Box<dyn std::error::Error>> {
    let (_git_dir, git) = fixture_git()?;
    let source_dir = tempfile::tempdir()?;
    let source = source_dir.path().canonicalize()?;
    let create = |args: Vec<OsString>, url: &str| {
        IsolatedCommand::crate_publish(
            Path::new("/bin/sh"),
            args,
            &source,
            &tool_env(),
            "fixture_git_987",
            CratePublishCredential::Trusted {
                request_url: url,
                request_token: "fixture_oidc_789",
            },
            &git,
        )
    };
    for url in [
        "http://pipelines.actions.githubusercontent.com/id",
        "https://pipelines.actions.githubusercontent.com.evil.invalid/id",
        "https://user@pipelines.actions.githubusercontent.com/id",
        "https://pipelines.actions.githubusercontent.com:443/id",
    ] {
        assert!(create(vec!["release".into()], url).is_err());
    }
    assert!(
        create(
            vec!["fixture_oidc_789".into()],
            "https://pipelines.actions.githubusercontent.com/id"
        )
        .is_err()
    );
    assert!(
        create(
            vec!["fixture_git_987".into()],
            "https://pipelines.actions.githubusercontent.com/id"
        )
        .is_err()
    );
    let mut hostile = tool_env();
    hostile.push(("CARGO_REGISTRY_TOKEN".into(), "unwanted".into()));
    assert!(
        IsolatedCommand::crate_publish(
            Path::new("/bin/sh"),
            vec!["release".into()],
            &source,
            &hostile,
            "fixture_git_987",
            CratePublishCredential::Bootstrap {
                cargo_token: "fixture_cargo_654"
            },
            &git,
        )
        .is_err()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn forged_gh_must_not_receive_authority() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt as _;
    if std::env::var_os("VELNOR_FORGED_GH_TEST").is_some() {
        let exec = crate::PinnedToolExec::new(
            vec![crate::PinnedTool::Gh],
            std::ffi::OsStr::new("gh"),
            vec!["--version".into()],
        )?;
        let _result = exec
            .command_with_github_token(&crate::ToolCatalog::pinned(), "synthetic-token")?
            .run_bounded(1024, Duration::from_secs(5));
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let marker = temp.path().join("executed");
    let mise = temp.path().join("mise");
    std::fs::write(
        &mise,
        format!(
            "#!/bin/sh\nif [ -n \"${{GH_TOKEN+x}}\" ]; then : > '{}'; fi\nprintf \"%s\\n\" '{}'\n",
            marker.display(),
            mise.display()
        ),
    )?;
    std::fs::set_permissions(&mise, std::fs::Permissions::from_mode(0o700))?;
    let path = std::env::join_paths([
        temp.path(),
        std::path::Path::new("/usr/bin"),
        std::path::Path::new("/bin"),
    ])?;
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "command::crate_tests::forged_gh_must_not_receive_authority",
        ])
        .env("VELNOR_FORGED_GH_TEST", "1")
        .env("PATH", path)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker.exists());
    Ok(())
}

#[test]
fn gh_cannot_be_combined_with_unverified_tool_or_payload() -> Result<(), Box<dyn std::error::Error>>
{
    let catalog = crate::ToolCatalog::pinned();
    let combined = crate::PinnedToolExec::new(
        vec![crate::PinnedTool::Gh, crate::PinnedTool::Rust],
        std::ffi::OsStr::new("gh"),
        vec!["--version".into()],
    )?;
    assert!(combined.command(&catalog).is_err());
    let shell = crate::PinnedToolExec::new(
        vec![crate::PinnedTool::Gh],
        std::ffi::OsStr::new("sh"),
        vec!["-c".into(), "gh --version".into()],
    )?;
    assert!(shell.command(&catalog).is_err());
    Ok(())
}

#[test]
fn gh_api_does_not_inherit_credentials_or_transport_overrides() {
    let parent = [
        ("PATH".into(), "/usr/bin".into()),
        ("HOME".into(), "/tmp/untrusted".into()),
        ("GH_TOKEN".into(), "dummy".into()),
        ("GITHUB_TOKEN".into(), "dummy".into()),
        ("HTTPS_PROXY".into(), "http://127.0.0.1:1234".into()),
        ("SSL_CERT_FILE".into(), "/tmp/fake-ca".into()),
    ];
    let child = crate::command::EnvPolicy::GhApi.child_env(&parent, &[]);
    for key in ["GH_TOKEN", "GITHUB_TOKEN", "HTTPS_PROXY", "SSL_CERT_FILE"] {
        assert!(!child.iter().any(|(name, _)| name == key), "{key}");
    }
}
