//! Unit cases for the publication config and tag grammar.

use super::{
    PublishBundle, PublishConfig, ReleaseTag, VersionSource, is_artifact_id, is_strict_semver,
};

fn bundle(id: &str) -> PublishBundle {
    PublishBundle {
        id: id.to_owned(),
        paths: vec!["schemas/schema.graphql".to_owned()],
        version: None,
        version_file: Some("Cargo.toml".to_owned()),
    }
}

fn problem(config: &PublishConfig) -> String {
    config
        .validate(".velnor/config.toml")
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default()
}

#[test]
fn tag_grammar_splits_at_the_last_version_marker() {
    let tag = ReleaseTag::parse("processor-shared-libs-v2.3.0");
    assert_eq!(
        tag.as_ref()
            .map(|t| (t.artifact.as_str(), t.version.as_str())),
        Some(("processor-shared-libs", "2.3.0"))
    );
    let tricky = ReleaseTag::parse("ui-v1-v4.0.1");
    assert_eq!(tricky.map(|t| t.artifact), Some("ui-v1".to_owned()));
    assert_eq!(
        ReleaseTag::parse("ui-v1.4.0").map(|t| t.render()),
        Some("ui-v1.4.0".to_owned())
    );
}

#[test]
fn tag_grammar_rejects_loose_shapes() {
    for bad in [
        "ui-v1.4",
        "ui-v1.4.0-rc1",
        "ui-v01.4.0",
        "ui-v1.4.0.0",
        "v1.4.0",
        "-v1.4.0",
        "UI-v1.4.0",
        "ui--v1.4.0",
        "ui_x-v1.4.0",
        "ui-1.4.0",
        "ui-v1.4.x",
    ] {
        assert!(ReleaseTag::parse(bad).is_none(), "{bad}");
    }
    assert!(is_strict_semver("0.0.0") && !is_strict_semver("1.2") && !is_strict_semver("1.2.03"));
    assert!(is_artifact_id("eth-migration") && !is_artifact_id("1eth") && !is_artifact_id("a-"));
}

#[test]
fn manifest_versions_read_only_literal_sources() {
    let cargo = "[workspace]\nmembers = []\n[dependencies]\nversion = \"9.9.9\"\n[workspace.package]\nversion = \"1.2.3\"\n";
    assert_eq!(
        VersionSource::CargoToml.read(cargo),
        Some("1.2.3".to_owned())
    );
    let inherited = "[package]\nname = \"a\"\nversion.workspace = true\n";
    assert_eq!(VersionSource::CargoToml.read(inherited), None);
    let package = "[package]\nname = \"a\"\nversion = \"0.4.0\" \n";
    assert_eq!(
        VersionSource::CargoToml.read(package),
        Some("0.4.0".to_owned())
    );
    assert_eq!(
        VersionSource::GradleProperties.read("org.x=1\nversion=2.3.0\n"),
        Some("2.3.0".to_owned())
    );
    assert_eq!(
        VersionSource::GradleProperties.read("version=2.3.0-SNAPSHOT\n"),
        None
    );
    assert_eq!(
        VersionSource::PackageJson.read("{\"version\":\"1.4.0\"}"),
        Some("1.4.0".to_owned())
    );
    assert_eq!(
        VersionSource::PackageJson.read("{\"version\":\"^1.4.0\"}"),
        None
    );
    assert_eq!(
        VersionSource::for_path("a/b/package.json"),
        Some(VersionSource::PackageJson)
    );
    assert_eq!(VersionSource::for_path("a/pom.xml"), None);
}

#[test]
fn valid_bundles_pass_and_defaults_are_empty() {
    assert!(PublishConfig::default().is_empty());
    let config = PublishConfig {
        bundles: vec![bundle("a-schema"), bundle("b-schema")],
        ..PublishConfig::default()
    };
    assert_eq!(problem(&config), "");
}

#[test]
fn invalid_bundles_name_their_key() {
    let mut inline = bundle("a");
    inline.version = Some("1.0.0".to_owned());
    assert!(
        problem(&PublishConfig {
            bundles: vec![inline],
            ..PublishConfig::default()
        })
        .contains("version_and_version_file")
    );
    let mut none = bundle("a");
    none.version_file = None;
    assert!(
        problem(&PublishConfig {
            bundles: vec![none],
            ..PublishConfig::default()
        })
        .contains("missing_version_source")
    );
    let mut loose = bundle("a");
    loose.version_file = None;
    loose.version = Some("1.0".to_owned());
    assert!(
        problem(&PublishConfig {
            bundles: vec![loose],
            ..PublishConfig::default()
        })
        .contains("bad_version")
    );
    let mut pom = bundle("a");
    pom.version_file = Some("pom.xml".to_owned());
    assert!(
        problem(&PublishConfig {
            bundles: vec![pom],
            ..PublishConfig::default()
        })
        .contains("unsupported_version_file")
    );
    assert!(
        problem(&PublishConfig {
            bundles: vec![bundle("A")],
            ..PublishConfig::default()
        })
        .contains("bad_artifact_id")
    );
    assert!(
        problem(&PublishConfig {
            bundles: vec![bundle("a"), bundle("a")],
            ..PublishConfig::default()
        })
        .contains("duplicate_id")
    );
    assert!(
        problem(&PublishConfig {
            bundles: vec![bundle("b"), bundle("a")],
            ..PublishConfig::default()
        })
        .contains("must_be_sorted_by_id")
    );
}

#[test]
fn invalid_paths_are_rejected() {
    for bad in [
        "",
        "/etc/passwd",
        "../x",
        "a/../b",
        "a//b",
        "a b",
        ".git/config",
        "a/*.txt",
    ] {
        let mut item = bundle("a");
        item.paths = vec![bad.to_owned()];
        assert!(
            problem(&PublishConfig {
                bundles: vec![item],
                ..PublishConfig::default()
            })
            .contains("unsafe_path"),
            "{bad}"
        );
    }
    let mut unsorted = bundle("a");
    unsorted.paths = vec!["b".to_owned(), "a".to_owned()];
    assert!(
        problem(&PublishConfig {
            bundles: vec![unsorted],
            ..PublishConfig::default()
        })
        .contains("must_be_sorted")
    );
    let mut dup = bundle("a");
    dup.paths = vec!["a".to_owned(), "a".to_owned()];
    assert!(
        problem(&PublishConfig {
            bundles: vec![dup],
            ..PublishConfig::default()
        })
        .contains("duplicate_path")
    );
    let mut empty = bundle("a");
    empty.paths = Vec::new();
    assert!(
        problem(&PublishConfig {
            bundles: vec![empty],
            ..PublishConfig::default()
        })
        .contains("bad_path_count")
    );
}
