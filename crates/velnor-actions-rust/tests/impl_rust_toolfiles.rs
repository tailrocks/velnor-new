//! Read-only `rust-toolchain.toml` inspection cases.
use velnor_actions_rust::{
    MISSING_RECOMMENDED_INPUT, TOOLING_INPUT_INVALID, ToolInspectError, inspect_toolchain_file,
    is_owned_tool_file, stack_for_symbol,
};

#[test]
fn symbols_route_to_owning_stacks() {
    for owned in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"] {
        assert_eq!(stack_for_symbol(owned), Some("rust"));
    }
    for foreign in ["mise.toml", "mise.lock"] {
        assert_eq!(stack_for_symbol(foreign), Some("mise"));
    }
    for unknown in ["actionlint.yaml", "package.json", "README.md", ""] {
        assert_eq!(stack_for_symbol(unknown), None);
    }
    assert_eq!(stack_for_symbol("nested/rust-toolchain.toml"), Some("rust"));
    assert_eq!(stack_for_symbol("nested/mise.toml"), Some("mise"));
}

#[test]
fn rust_inspects_only_its_tool_file() {
    assert!(is_owned_tool_file("rust-toolchain.toml"));
    assert!(is_owned_tool_file("nested/rust-toolchain.toml"));
    assert!(!is_owned_tool_file("mise.toml"));
    assert!(!is_owned_tool_file("mise.lock"));
    assert!(!is_owned_tool_file("Cargo.toml"));
    let result = inspect_toolchain_file("mise.toml", Some("[tools]\n"));
    assert!(matches!(result, Err(ToolInspectError::NotOwned { .. })));
    let result = inspect_toolchain_file("mise.lock", None);
    assert!(matches!(result, Err(ToolInspectError::NotOwned { .. })));
}

#[test]
fn missing_toolchain_yields_recommendation_not_failure() {
    let inspection = inspect_toolchain_file("rust-toolchain.toml", None);
    let Ok(inspection) = inspection else {
        panic!("missing tool file must not fail");
    };
    assert!(inspection.spec.is_none());
    assert_eq!(inspection.findings.len(), 1);
    let finding = &inspection.findings[0];
    assert_eq!(finding.code, MISSING_RECOMMENDED_INPUT);
    assert_eq!(finding.file, "rust-toolchain.toml");
    assert!(finding.observed.is_none());
    assert!(finding.recommendation.contains("rust-toolchain.toml"));
    assert!(finding.recommendation.contains("never creates"));
}

#[test]
fn malformed_toolchain_yields_invalid_finding() {
    for (label, content) in [
        ("unterminated section", "[toolchain\nchannel = \"stable\"\n"),
        ("bare line", "[toolchain]\njust words here\n"),
        ("unterminated string", "[toolchain]\nchannel = \"stable\n"),
        ("bare channel", "[toolchain]\nchannel = stable\n"),
        (
            "unterminated list",
            "[toolchain]\nchannel = \"stable\"\ncomponents = [\"clippy\"\n",
        ),
        (
            "non-string item",
            "[toolchain]\nchannel = \"stable\"\ncomponents = [clippy]\n",
        ),
    ] {
        let inspection = inspect_toolchain_file("rust-toolchain.toml", Some(content));
        let Ok(inspection) = inspection else {
            panic!("{label} must not fail");
        };
        assert!(inspection.spec.is_none(), "{label} must yield no spec");
        assert_eq!(inspection.findings.len(), 1, "{label}");
        let finding = &inspection.findings[0];
        assert_eq!(finding.code, TOOLING_INPUT_INVALID, "{label}");
        assert_eq!(finding.file, "rust-toolchain.toml", "{label}");
        assert!(finding.observed.is_some(), "{label}");
        assert!(finding.recommendation.contains("manually"), "{label}");
    }
}

#[test]
fn valid_toolchain_extracts_spec_without_findings() {
    let content = "# project pin\n[toolchain]\nchannel = \"1.91.0\"\n\
         components = [\"rustfmt\", \"clippy\", \"clippy\"]\n\
         targets = [\"wasm32-unknown-unknown\"]\n\
         profile = \"minimal\"\n[other]\nignored = true\n";
    let inspection = inspect_toolchain_file("rust-toolchain.toml", Some(content));
    let Ok(inspection) = inspection else {
        panic!("valid tool file must inspect cleanly");
    };
    assert_eq!(
        inspection.findings,
        [] as [velnor_actions_rust::ToolFinding; 0]
    );
    let Some(spec) = inspection.spec else {
        panic!("valid tool file must yield a spec");
    };
    assert_eq!(spec.channel.as_deref(), Some("1.91.0"));
    assert_eq!(
        spec.components,
        vec!["clippy".to_owned(), "rustfmt".to_owned()]
    );
    assert_eq!(spec.targets, vec!["wasm32-unknown-unknown".to_owned()]);
}

#[test]
fn toolchain_without_channel_recommends_pin() {
    for content in [
        "[toolchain]\ncomponents = [\"clippy\"]\n",
        "[toolchain]\nchannel = \"\"\n",
        "[profile]\nname = \"x\"\n",
        "",
    ] {
        let inspection = inspect_toolchain_file("nested/rust-toolchain.toml", Some(content));
        let Ok(inspection) = inspection else {
            panic!("unpinned tool file must not fail");
        };
        assert!(inspection.spec.is_some());
        assert_eq!(inspection.findings.len(), 1);
        let finding = &inspection.findings[0];
        assert_eq!(finding.code, MISSING_RECOMMENDED_INPUT);
        assert_eq!(finding.file, "nested/rust-toolchain.toml");
        assert!(finding.recommendation.contains("channel"));
    }
}

#[test]
fn every_finding_names_supplying_file() {
    let cases = [
        ("rust-toolchain.toml", None),
        ("rust-toolchain.toml", Some("[toolchain\n")),
        ("nested/rust-toolchain.toml", Some("[toolchain]\n")),
    ];
    for (path, content) in cases {
        let inspection = inspect_toolchain_file(path, content);
        let Ok(inspection) = inspection else {
            panic!("inspection must not fail for {path}");
        };
        assert!(!inspection.findings.is_empty(), "{path} must report");
        for finding in &inspection.findings {
            assert_eq!(finding.file, path);
            assert_ne!(finding.code, "");
            assert_ne!(finding.recommendation, "");
        }
    }
}
