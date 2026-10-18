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
    let parent = [
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
        (OsString::from("PATH"), OsString::from("/usr/bin")),
    ];
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
    assert!(!exec.args().iter().any(|arg| arg == "ghs_app_token"));
    assert!(!format!("{exec:?} {command:?}").contains("ghs_app_token"));
}

#[test]
fn app_token_rejects_non_gh_programs_and_mixed_tools() {
    for (tools, program) in [
        (vec![PinnedTool::Gh], "git"),
        (vec![PinnedTool::Gh, PinnedTool::Actionlint], "gh"),
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
