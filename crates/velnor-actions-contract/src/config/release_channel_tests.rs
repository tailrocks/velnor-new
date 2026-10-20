use super::{ReleaseAuthentication, ReleaseRegistry, RustReleaseConfig};

#[test]
fn registry_publication_requires_an_explicit_crates_io_selector() {
    let implicit: RustReleaseConfig =
        serde_json::from_str(r#"{"enabled":true,"packages":["jackin"]}"#).expect("git-only policy");
    assert_eq!(implicit.registry, ReleaseRegistry::GitTag);
    assert!(implicit.validate("config.toml").is_ok());
    let explicit: RustReleaseConfig =
        serde_json::from_str(r#"{"enabled":true,"packages":["jackin"],"registry":"crates-io"}"#)
            .expect("explicit registry policy");
    assert_eq!(explicit.registry, ReleaseRegistry::CratesIo);
    assert!(explicit.validate("config.toml").is_ok());
    let mut invalid = implicit;
    invalid.authentication = Some(ReleaseAuthentication::TrustedPublishing);
    assert!(invalid.validate("config.toml").is_err());
}

#[test]
fn native_stable_tag_template_is_closed_without_weakening_package_tag_validation() {
    for tag in [
        "v{{version}}",
        "v{{ version }}",
        "{{ package }}-v{{ version }}",
    ] {
        let config = RustReleaseConfig {
            tag_name: tag.to_owned(),
            ..RustReleaseConfig::default()
        };
        assert!(config.validate("config.toml").is_ok(), "{tag}");
    }
    for tag in [
        "v {{version}}",
        "{{version}}",
        "other-{{version}}",
        "v{{ version }}-latest",
        "v{{ github.sha }}",
    ] {
        let config = RustReleaseConfig {
            tag_name: tag.to_owned(),
            ..RustReleaseConfig::default()
        };
        assert!(config.validate("config.toml").is_err(), "{tag}");
    }
}
