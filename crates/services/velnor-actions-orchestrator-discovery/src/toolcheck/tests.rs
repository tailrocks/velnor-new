use super::*;

#[test]
fn checks_report_presence_parse_values_and_digests() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.85.0\"\ncomponents = [\"clippy\"]\n",
    )
    .expect("write");
    std::fs::write(dir.path().join("mise.toml"), "[tools]\nrust = \"1.85.0\"\n").expect("write");
    std::fs::write(dir.path().join("mise.lock"), "{}\n").expect("write");
    let checks = check_tool_inputs(dir.path());
    assert_eq!(checks.len(), 3);
    for check in &checks {
        assert!(check.present, "{}", check.path);
        assert_eq!(check.parse, ToolParse::Valid, "{}", check.path);
        assert!(
            check
                .digest
                .as_deref()
                .is_some_and(|d| d.starts_with("b3-"))
        );
    }
    let toolchain = checks
        .iter()
        .find(|c| c.path == "rust-toolchain.toml")
        .expect("t");
    assert_eq!(
        toolchain.values.get("channel").map(String::as_str),
        Some("1.85.0")
    );
    let mise = checks.iter().find(|c| c.path == "mise.toml").expect("m");
    assert_eq!(
        mise.values.get("tools.rust").map(String::as_str),
        Some("1.85.0")
    );
    let missing = check_tool_inputs(std::path::Path::new("/nonexistent-root-velnor"));
    assert!(missing.iter().all(|check| !check.present));
    assert!(
        missing
            .iter()
            .all(|check| check.parse == ToolParse::Missing)
    );
}

#[test]
fn malformed_files_are_invalid_with_digests() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("mise.toml"), "[tools\nrust = \n").expect("write");
    let checks = check_tool_inputs(dir.path());
    let mise = checks.iter().find(|c| c.path == "mise.toml").expect("m");
    assert!(matches!(mise.parse, ToolParse::Invalid { .. }));
    assert!(mise.digest.is_some());
}

#[test]
fn unreadable_files_are_not_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A directory where the tool file belongs: `read` fails with a
    // non-`NotFound` IO error on every platform, even for root.
    std::fs::create_dir(dir.path().join("mise.toml")).expect("dir");
    let checks = check_tool_inputs(dir.path());
    let mise = checks.iter().find(|c| c.path == "mise.toml").expect("m");
    assert!(!mise.present, "no bytes were readable");
    assert_eq!(
        mise.parse,
        ToolParse::Unreadable,
        "inaccessible, not absent"
    );
    assert!(mise.digest.is_none(), "no digest without bytes");
    assert!(
        mise.codes.contains(&TOOLING_INPUT_INVALID.to_owned()),
        "flagged: {:?}",
        mise.codes
    );
    let lock = checks.iter().find(|c| c.path == "mise.lock").expect("l");
    assert_eq!(lock.parse, ToolParse::Missing, "absent stays missing");
    let lines = crate::toolfindings::tool_check_lines(&checks);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("mise.toml") && line.contains("unreadable")),
        "unreadable surfaces a line: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("mise.lock")),
        "missing stays silent: {lines:?}"
    );
}
