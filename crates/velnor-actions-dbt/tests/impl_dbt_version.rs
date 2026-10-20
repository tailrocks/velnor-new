use velnor_actions_dbt::{VersionError, parse_fusion_version};

#[test]
fn exact_preview_versions_parse_and_round_trip() {
    for text in ["2.0.0-preview.219", "2.10.3-preview.0", "0.1.0-preview.7"] {
        let version = parse_fusion_version(text).expect(text);
        assert_eq!(version.to_string(), text);
    }
    let a = parse_fusion_version("2.0.0-preview.9").expect("a");
    let b = parse_fusion_version("2.0.0-preview.10").expect("b");
    assert!(a < b);
}

#[test]
fn everything_else_is_not_exact() {
    for text in [
        "2.0.0",
        "latest",
        "2.0",
        "2.0.0-preview",
        "2.0.0-preview.",
        "2.0.0-preview.01",
        "2.0.0-beta.1",
        "02.0.0-preview.1",
        "2.0.0-preview.1.2",
        "2.0.0.1-preview.1",
        "~2.0.0-preview.1",
        "v2.0.0-preview.1",
        "{{version}}",
        "2.0.0-preview.1\n",
    ] {
        let error = parse_fusion_version(text).expect_err(text);
        assert_eq!(error.code(), "project_tool_version_not_exact", "{text}");
    }
    assert_eq!(
        parse_fusion_version("$GITHUB_ENV").expect_err("charset"),
        VersionError::Charset
    );
}
