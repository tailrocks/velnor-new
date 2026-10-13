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
fn declared_nested_mise_sources_are_read_and_hashed_by_exact_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let native = dir.path().join("native");
    std::fs::create_dir(&native).expect("native dir");
    let config = "[tasks.desktop-format-check]\nrun = \"echo format\"\n";
    let lock = "[tools]\n";
    let toolchain = "[toolchain]\nchannel = \"1.99.0\"\n";
    std::fs::write(native.join("mise.toml"), config).expect("native mise config");
    std::fs::write(native.join("mise.lock"), lock).expect("native mise lock");
    std::fs::write(native.join("rust-toolchain.toml"), toolchain).expect("native rust toolchain");

    let paths = [
        "native/mise.toml".to_owned(),
        "native/mise.lock".to_owned(),
        "native/rust-toolchain.toml".to_owned(),
    ];
    let checks = check_tool_inputs_with_paths(dir.path(), &paths);
    assert_eq!(checks.len(), 6, "root inputs plus three declared sources");
    let config_check = checks
        .iter()
        .find(|check| check.path == "native/mise.toml")
        .expect("nested config check");
    assert_eq!(config_check.parse, ToolParse::Valid);
    assert!(config_check.native.is_some());
    assert_eq!(
        config_check
            .values
            .get("tasks.desktop-format-check.run")
            .map(String::as_str),
        Some("echo format")
    );
    let lock_check = checks
        .iter()
        .find(|check| check.path == "native/mise.lock")
        .expect("nested lock check");
    assert_eq!(lock_check.parse, ToolParse::Valid);
    assert!(lock_check.native.is_some());
    let rust_check = checks
        .iter()
        .find(|check| check.path == "native/rust-toolchain.toml")
        .expect("nested toolchain check");
    assert_eq!(rust_check.parse, ToolParse::Valid);
    assert_eq!(
        rust_check.values.get("channel").map(String::as_str),
        Some("1.99.0")
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

#[test]
#[cfg(unix)]
fn symlinked_tool_input_is_rejected_without_following() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside tempdir");
    std::fs::write(
        outside.path().join("mise.toml"),
        "[tools]\nrust = \"1.85.0\"\n",
    )
    .expect("write outside tool config");
    std::os::unix::fs::symlink(
        outside.path().join("mise.toml"),
        dir.path().join("mise.toml"),
    )
    .expect("symlink tool config");

    let checks = check_tool_inputs(dir.path());
    let mise = checks
        .iter()
        .find(|check| check.path == "mise.toml")
        .expect("mise");
    assert_eq!(mise.parse, ToolParse::Unreadable);
    assert!(mise.native.is_none());
    assert!(mise.digest.is_none());
}

#[test]
fn oversized_tool_input_is_rejected_before_parsing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let max = usize::try_from(crate::safe_read::MAX_REPO_FILE_BYTES).expect("limit fits");
    let bytes = vec![b'x'; max + 1];
    std::fs::write(dir.path().join("mise.lock"), bytes).expect("write oversized lock");

    let checks = check_tool_inputs(dir.path());
    let lock = checks
        .iter()
        .find(|check| check.path == "mise.lock")
        .expect("lock");
    assert_eq!(lock.parse, ToolParse::Unreadable);
    assert!(lock.native.is_none());
    assert!(lock.digest.is_none());
}
