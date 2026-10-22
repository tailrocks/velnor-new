use crate::command::PROXY_ENV_KEYS;
use crate::{PinnedTool, PinnedToolExec, ToolCatalog};
use std::ffi::{OsStr, OsString};

#[test]
fn app_token_is_explicit_and_scrubs_ambient_credentials() {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        vec![OsString::from("api"), OsString::from("user")],
    )
    .expect("gh command");
    let command = exec
        .command_with_github_token(&ToolCatalog::pinned(), "ghs_app_token")
        .expect("App token command");
    let parent = poisoned_github_parent();
    let child = command.spawn_env(&parent);
    let credentials: Vec<_> = child
        .iter()
        .filter(|(key, _)| {
            key == "GH_TOKEN"
                || key == "GITHUB_TOKEN"
                || key == "VELNOR_TAP_TOKEN"
                || key == "APP_PRIVATE_KEY"
        })
        .cloned()
        .collect();
    assert_eq!(
        credentials,
        vec![(OsString::from("GH_TOKEN"), OsString::from("ghs_app_token"))]
    );
    let expected = expected_github_child_env(&command, true);
    assert_eq!(child, expected);
    let without_data_dir: Vec<_> = parent
        .iter()
        .filter(|(key, _)| key != "MISE_DATA_DIR")
        .cloned()
        .collect();
    assert_eq!(
        command.spawn_env(&without_data_dir),
        expected_github_child_env(&command, false)
    );
    assert!(!exec.args().iter().any(|arg| arg == "ghs_app_token"));
    assert!(!format!("{exec:?} {command:?}").contains("ghs_app_token"));
}

fn poisoned_github_parent() -> Vec<(OsString, OsString)> {
    let mut parent = vec![
        (OsString::from("PATH"), OsString::from("/custom/mise/bin")),
        (OsString::from("HOME"), OsString::from("/custom/home")),
        (
            OsString::from("MISE_DATA_DIR"),
            OsString::from("/custom/mise/data"),
        ),
        (
            OsString::from("MISE_CONFIG_FILE"),
            OsString::from("/poison/poison-config"),
        ),
        (OsString::from("MISE_ENV"), OsString::from("poison")),
        (
            OsString::from("MISE_TRUSTED_CONFIG_PATHS"),
            OsString::from("/poison"),
        ),
        (
            OsString::from("MISE_RUSTUP_HOME"),
            OsString::from("/poison/rustup"),
        ),
        (
            OsString::from("MISE_CARGO_HOME"),
            OsString::from("/poison/cargo"),
        ),
        (
            OsString::from("XDG_CONFIG_HOME"),
            OsString::from("/poison/xdg"),
        ),
        (
            OsString::from("MISE_GITHUB_TOKEN"),
            OsString::from("mise-token"),
        ),
        (
            OsString::from("CARGO_REGISTRIES_CRATES_IO_TOKEN"),
            OsString::from("registry-token"),
        ),
        (OsString::from("GH_TOKEN"), OsString::from("source-token")),
        (
            OsString::from("GITHUB_TOKEN"),
            OsString::from("source-token-2"),
        ),
        (
            OsString::from("VELNOR_TAP_TOKEN"),
            OsString::from("app-token"),
        ),
        (
            OsString::from("APP_PRIVATE_KEY"),
            OsString::from("private-key"),
        ),
        (
            OsString::from("GH_HOST"),
            OsString::from("attacker.invalid"),
        ),
        (
            OsString::from("GH_CONFIG_DIR"),
            OsString::from("/poison/gh"),
        ),
    ];
    parent.extend(
        PROXY_ENV_KEYS
            .iter()
            .map(|key| (OsString::from(key), OsString::from("proxy-bait"))),
    );
    parent
}

fn expected_github_child_env(
    command: &crate::command::IsolatedCommand,
    include_data_dir: bool,
) -> Vec<(OsString, OsString)> {
    let mut expected = vec![
        (OsString::from("PATH"), OsString::from("/custom/mise/bin")),
        (OsString::from("HOME"), OsString::from("/custom/home")),
    ];
    if include_data_dir {
        expected.push((
            OsString::from("MISE_DATA_DIR"),
            OsString::from("/custom/mise/data"),
        ));
    }
    expected.extend(command.full_env());
    expected
}

#[test]
fn app_token_rejects_non_gh_programs_and_mixed_tools() {
    for (tools, program) in [
        (vec![PinnedTool::Gh], "git"),
        (vec![PinnedTool::Gh, PinnedTool::Jactionlint], "gh"),
    ] {
        let exec = PinnedToolExec::new(tools, OsStr::new(program), vec![OsString::from("api")])
            .expect("valid pinned execution");
        assert!(
            exec.command_with_github_token(&ToolCatalog::pinned(), "ghs_token")
                .is_err()
        );
    }
}

#[test]
fn app_token_rejects_empty_or_malformed_token() {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        vec![OsString::from("api")],
    )
    .expect("gh command");
    for token in ["", "token with spaces", "token\nvalue"] {
        assert!(
            exec.command_with_github_token(&ToolCatalog::pinned(), token)
                .is_err()
        );
    }
}
