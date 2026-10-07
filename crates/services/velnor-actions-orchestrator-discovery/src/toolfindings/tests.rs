use super::*;
use crate::toolcheck::check_tool_inputs;

/// Checks over a scratch root carrying `files`.
fn checks_for(files: &[(&str, &str)]) -> Vec<ToolInputCheck> {
    let dir = tempfile::tempdir().expect("tempdir");
    for (path, content) in files {
        std::fs::write(dir.path().join(path), content).expect("write");
    }
    check_tool_inputs(dir.path())
}

#[test]
fn conflicts_need_comparable_divergent_pins() {
    let checks = checks_for(&[
        ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.84.0\"\n"),
        ("mise.toml", "[tools]\nrust = \"1.85.0\"\n"),
    ]);
    let findings = tool_conflicts(&checks);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].code, CONFLICTING_TOOL_VALUES);
    assert!(findings[0].validate().is_ok());
    assert!(finding_line(&findings[0]).contains("1.84.0"));
    let aligned = checks_for(&[
        ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.84.0\"\n"),
        ("mise.toml", "[tools]\nrust = \"1.84.0\"\n"),
    ]);
    assert!(tool_conflicts(&aligned).is_empty());
    let mixed = checks_for(&[
        ("rust-toolchain.toml", "[toolchain]\nchannel = \"stable\"\n"),
        ("mise.toml", "[tools]\nrust = \"1.84.0\"\n"),
    ]);
    assert!(tool_conflicts(&mixed).is_empty());
}

#[test]
fn unrecognized_values_are_unsupported() {
    let checks = checks_for(&[(
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"frobnicator\"\n",
    )]);
    let findings = tool_conflicts(&checks);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].code, UNSUPPORTED_TOOL_VALUE);
    assert!(findings[0].validate().is_ok());
}
