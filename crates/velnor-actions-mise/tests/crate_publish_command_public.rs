// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{ffi::OsString, path::Path, time::Duration};
use velnor_actions_mise::command::{CratePublishCredential, IsolatedCommand};

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
    let script = "test -n \"$GIT_TOKEN\"; test \"$GITHUB_ACTIONS\" = true; test -n \"$ACTIONS_ID_TOKEN_REQUEST_URL\"; test -n \"$ACTIONS_ID_TOKEN_REQUEST_TOKEN\"; test -z \"${CARGO_REGISTRY_TOKEN+x}\"; test -z \"${HTTP_PROXY+x}\"; test -z \"${HOSTILE_TOKEN+x}\"";
    let command = IsolatedCommand::crate_publish(
        Path::new("/bin/sh"),
        vec!["-c".into(), script.into()],
        Path::new("/tmp"),
        &tool_env(),
        "fixture_git_987",
        CratePublishCredential::Trusted {
            request_url: "https://pipelines.actions.githubusercontent.com/id",
            request_token: "fixture_oidc_789",
        },
    )?;
    let debug = format!("{command:?}");
    assert!(!debug.contains("fixture_git_987") && !debug.contains("fixture_oidc_789"));
    assert!(command.run_bounded(1024, Duration::from_secs(5))?.success);
    let parent = vec![
        ("HOSTILE_TOKEN".into(), "unwanted".into()),
        ("HTTP_PROXY".into(), "http://untrusted".into()),
    ];
    let env = command.spawn_env(&parent);
    assert!(!env.iter().any(|(key, _)| key == "HOSTILE_TOKEN"
        || key == "HTTP_PROXY"
        || key == "CARGO_REGISTRY_TOKEN"));
    let bootstrap = IsolatedCommand::crate_publish(Path::new("/bin/sh"), vec!["-c".into(), "test -n \"$GIT_TOKEN\"; test -n \"$CARGO_REGISTRY_TOKEN\"; test -z \"${GITHUB_ACTIONS+x}\"; test -z \"${ACTIONS_ID_TOKEN_REQUEST_TOKEN+x}\"".into()], Path::new("/tmp"), &tool_env(), "fixture_git_987", CratePublishCredential::Bootstrap { cargo_token: "fixture_cargo_654" })?;
    assert!(bootstrap.run_bounded(1024, Duration::from_secs(5))?.success);
    assert!(!format!("{bootstrap:?}").contains("fixture_cargo_654"));
    Ok(())
}

#[test]
fn crate_publish_rejects_untrusted_endpoints_and_secret_arguments() {
    let create = |args: Vec<OsString>, url: &str| {
        IsolatedCommand::crate_publish(
            Path::new("/bin/sh"),
            args,
            Path::new("/tmp"),
            &tool_env(),
            "fixture_git_987",
            CratePublishCredential::Trusted {
                request_url: url,
                request_token: "fixture_oidc_789",
            },
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
            Path::new("/tmp"),
            &hostile,
            "fixture_git_987",
            CratePublishCredential::Bootstrap {
                cargo_token: "fixture_cargo_654"
            }
        )
        .is_err()
    );
}
