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
        .filter(|(key, _)| key != "MISE_DATA_DIR" && key != "XDG_DATA_HOME")
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
            OsString::from("XDG_DATA_HOME"),
            OsString::from("/custom/xdg/data"),
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
fn app_token_uses_only_the_selected_mise_data_root() {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        vec![OsString::from("api")],
    )
    .expect("gh command");
    let command = exec
        .command_with_github_token(&ToolCatalog::pinned(), "ghs_test_token")
        .expect("App token command");
    let child_dir = |explicit: Option<&str>, xdg: Option<&str>| {
        let mut parent = vec![
            (OsString::from("PATH"), OsString::from("/runner/bin")),
            (OsString::from("HOME"), OsString::from("/runner/home")),
        ];
        if let Some(value) = explicit {
            parent.push((OsString::from("MISE_DATA_DIR"), OsString::from(value)));
        }
        if let Some(value) = xdg {
            parent.push((OsString::from("XDG_DATA_HOME"), OsString::from(value)));
        }
        command
            .spawn_env(&parent)
            .into_iter()
            .find(|(key, _)| key == "MISE_DATA_DIR")
            .map(|(_, value)| value)
    };

    assert_eq!(
        child_dir(Some("/explicit/mise"), Some("/xdg")),
        Some(OsString::from("/explicit/mise"))
    );
    assert_eq!(
        child_dir(None, Some("/xdg")),
        Some(OsString::from("/xdg/mise"))
    );
    assert_eq!(
        child_dir(Some(""), Some("/xdg")),
        Some(OsString::from("/xdg/mise"))
    );
    assert_eq!(child_dir(None, None), None);
    assert_eq!(child_dir(Some(""), Some("")), None);
    let duplicate_explicit = command.spawn_env(&[
        (OsString::from("MISE_DATA_DIR"), OsString::from("/stale")),
        (OsString::from("MISE_DATA_DIR"), OsString::new()),
        (OsString::from("XDG_DATA_HOME"), OsString::from("/xdg")),
    ]);
    assert!(
        duplicate_explicit
            .contains(&(OsString::from("MISE_DATA_DIR"), OsString::from("/xdg/mise")))
    );
}

#[cfg(unix)]
#[test]
fn app_token_preserves_non_utf8_xdg_cache_paths() {
    use std::os::unix::ffi::OsStringExt;

    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        vec![OsString::from("api")],
    )
    .expect("gh command");
    let command = exec
        .command_with_github_token(&ToolCatalog::pinned(), "ghs_test_token")
        .expect("App token command");
    let xdg = OsString::from_vec(b"/runner/data-\xff".to_vec());
    let child = command.spawn_env(&[
        (OsString::from("HOME"), OsString::from("/runner/home")),
        (OsString::from("XDG_DATA_HOME"), xdg.clone()),
    ]);

    assert!(child.contains(&(
        OsString::from("MISE_DATA_DIR"),
        std::path::PathBuf::from(xdg).join("mise").into_os_string()
    )));
    assert!(!child.iter().any(|(key, _)| key == "XDG_DATA_HOME"));
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
    for token in [
        "",
        "token with spaces",
        "token\nvalue",
        "a\0b",
        "a\u{7f}b",
        "é",
    ] {
        let error = exec
            .command_with_github_token(&ToolCatalog::pinned(), token)
            .expect_err("malformed credential rejected");
        let display = error.to_string();
        assert!(display.contains("GH_TOKEN"));
        assert!(display.contains("credential_rejected"));
        assert!(token.is_empty() || !display.contains(token));
    }
}

#[test]
fn shared_transport_predicate_accepts_only_opaque_safe_ascii_bytes() {
    use crate::command::is_safe_credential_value;

    assert!(is_safe_credential_value("legacy-token"));
    assert!(is_safe_credential_value("legacy.opaque_token-v2"));
    assert!(!is_safe_credential_value(""));
    for value in ["a b", "a\tb", "a\nb", "a\0b", "a\u{7f}b", "é", "a/b", "a+b"] {
        assert!(!is_safe_credential_value(value), "{value:?}");
    }
}

#[test]
fn app_token_accepts_legacy_and_long_dotted_opaque_values() {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        vec![OsString::from("api")],
    )
    .expect("gh command");
    for token in [
        "legacy-token",
        &format!("legacy.{}.suffix", "x".repeat(600)),
    ] {
        let command = exec
            .command_with_github_token(&ToolCatalog::pinned(), token)
            .expect("opaque token accepted");
        assert!(
            command
                .full_env()
                .contains(&(OsString::from("GH_TOKEN"), OsString::from(token)))
        );
        assert!(!exec.args().iter().any(|arg| arg == token));
        assert!(!command.argv().iter().any(|arg| arg == token));
        assert!(!format!("{exec:?} {command:?}").contains(token));
        let child = command.spawn_env(&poisoned_github_parent());
        assert_eq!(child.iter().filter(|(key, _)| key == "GH_TOKEN").count(), 1);
        assert_eq!(child, expected_github_child_env(&command, true));
        assert!(!child.iter().any(|(key, value)| {
            key != "GH_TOKEN" && value.to_string_lossy().contains("source-token")
        }));
    }
}

#[test]
fn dotted_token_does_not_expand_github_token_command_authority() {
    for (tools, program) in [
        (vec![PinnedTool::Gh], "git"),
        (vec![PinnedTool::Gh, PinnedTool::Jactionlint], "gh"),
    ] {
        let exec = PinnedToolExec::new(tools, OsStr::new(program), vec![OsString::from("api")])
            .expect("valid pinned execution");
        assert!(
            exec.command_with_github_token(&ToolCatalog::pinned(), "legacy.token.v1")
                .is_err()
        );
    }
}
