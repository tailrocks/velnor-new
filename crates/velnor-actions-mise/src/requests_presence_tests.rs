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
fn missing_row_observation_is_explicit_and_allowlisted() {
    let (installed, observation) = installed_versions_with_observation(
        r#"[{"version":"1.98.1","installed":false,"broken":true,"install_path":"/mise/installs/rust/1.98.1","symlinked_to":"/mise/shims/rust","secret":"do-not-report"}]"#,
        "1.99.1",
    )
    .expect("valid rows");

    assert_eq!(installed, Vec::<String>::new());
    assert_eq!(observation.requested_version, "1.99.1");
    assert!(observation.requested_row.is_none());
    assert_eq!(observation.other_rows.len(), 1);
    let report = observation.to_json_line();
    assert!(report.contains("/mise/installs/rust/1.98.1"));
    assert!(!report.contains("do-not-report"));
}

#[test]
fn false_installed_requested_row_and_broken_metadata_are_retained() {
    let (installed, observation) = installed_versions_with_observation(
        r#"[{"version":"1.99.1","installed":false,"broken":true,"install_path":"/mise/rust/1.99.1"}]"#,
        "1.99.1",
    )
    .expect("valid rows");

    assert_eq!(installed, Vec::<String>::new());
    let requested = observation.requested_row.expect("requested row retained");
    assert_eq!(requested.installed, Some(false));
    assert_eq!(requested.broken, Some(true));
}

#[test]
fn observation_rows_strings_and_json_are_bounded_and_unknown_keys_excluded() {
    let rows = (0..20)
        .map(|index| {
            serde_json::json!({
                "version": format!("1.{index}.0{}", "\\\"".repeat(200)),
                "installed": false,
                "broken": true,
                "install_path": "x".repeat(600),
                "symlinked_to": "y".repeat(600),
                "secret": "private-value"
            })
        })
        .collect::<Vec<_>>();
    let json = serde_json::to_string(&rows).expect("serialize test rows");
    let (_, observation) =
        installed_versions_with_observation(&json, "requested-not-listed").expect("valid rows");

    assert_eq!(observation.other_rows.len(), MAX_OBSERVATION_OTHER_ROWS);
    assert!(observation.other_rows_truncated);
    assert!(observation.to_json_line().len() <= MAX_OBSERVATION_JSON_BYTES);
    assert!(!observation.to_json_line().contains("private-value"));
    for row in &observation.other_rows {
        for value in [
            row.version.as_deref(),
            row.install_path.as_deref(),
            row.symlinked_to.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            assert!(value.len() <= MAX_OBSERVATION_STRING_BYTES);
        }
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
