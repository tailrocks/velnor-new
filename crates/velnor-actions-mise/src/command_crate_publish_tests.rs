// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
pub(super) fn crate_publish_exposes_only_the_selected_credentials()
-> Result<(), Box<dyn std::error::Error>> {
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
pub(super) fn crate_publish_rejects_git_tools_inside_source()
-> Result<(), Box<dyn std::error::Error>> {
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
pub(super) fn crate_publish_rejects_untrusted_endpoints_and_secret_arguments()
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
pub(super) fn forged_gh_must_not_receive_authority() -> Result<(), Box<dyn std::error::Error>> {
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
pub(super) fn gh_cannot_be_combined_with_unverified_tool_or_payload()
-> Result<(), Box<dyn std::error::Error>> {
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
