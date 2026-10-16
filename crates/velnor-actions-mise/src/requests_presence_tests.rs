//! Presence-preflight tests: query shape, parsing, and fail-closed gaps.

use super::*;

#[test]
fn empty_tool_list_is_rejected() {
    let err = ToolPresence::new(Vec::new()).expect_err("zero tools must fail");
    assert!(matches!(err, MiseError::EmptyToolchain), "got {err}");
}

#[test]
fn query_is_read_only_ls_json_under_exec_isolation() {
    let presence = ToolPresence::new(vec![PinnedTool::Rust]).expect("tools");
    let command = presence.command(PinnedTool::Rust);
    let argv: Vec<String> = command
        .argv()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        argv,
        [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "ls",
            "--json",
            "rust"
        ]
    );
    assert!(
        command.disables_auto_install(),
        "query must never install: {command:?}"
    );
}

#[test]
fn installed_versions_keeps_installed_entries_sorted() {
    let parsed = installed_versions(
        r#"[
            {"version": "1.98.1", "installed": true},
            {"version": "1.94.0", "installed": false},
            {"version": "1.95.0", "installed": true}
        ]"#,
    )
    .expect("parse");
    assert_eq!(parsed, vec!["1.95.0".to_owned(), "1.98.1".to_owned()]);
    let empty = installed_versions("[]").expect("parse empty");
    assert!(empty.is_empty(), "no entries means no installed versions");
}

#[test]
fn shape_drift_fails_closed_unparsable() {
    for body in ["not json", r#"{"rust": []}"#, r#"[{"nope": true}]"#] {
        let err = installed_versions(body).expect_err("drift must fail closed");
        assert!(err.to_string().contains("mise_ls_unparsable"), "got {err}");
    }
}

#[test]
fn missing_pin_names_spec_and_installed_versions() {
    let err = MiseError::MissingToolPin {
        spec: "rust@9.9.9".to_owned(),
        installed: vec!["1.94.0".to_owned()],
    };
    assert_eq!(
        err.to_string(),
        "missing_tool_pin: rust@9.9.9 not installed (installed: 1.94.0)"
    );
    let uninstalled = MiseError::MissingToolPin {
        spec: "rust@9.9.9".to_owned(),
        installed: Vec::new(),
    };
    assert_eq!(
        uninstalled.to_string(),
        "missing_tool_pin: rust@9.9.9 not installed (installed: none)"
    );
}

/// Same live-mise assumption as the validator tests: the bootstrap
/// installed the pinned catalog, so presence holds on this machine.
#[test]
fn pinned_catalog_satisfies_presence() {
    let presence = ToolPresence::new(vec![PinnedTool::Rust]).expect("tools");
    presence
        .run(&ToolCatalog::pinned())
        .expect("bootstrap installs the pinned rust toolchain");
}
