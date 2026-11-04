// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::config::{
    RecoveryRelease, ReleaseAuthentication, ReleaseRegistry, RustReleaseConfig,
};

#[test]
fn bootstrap_versions_follow_cargo_semver() {
    let mut release = RustReleaseConfig {
        enabled: true,
        registry: ReleaseRegistry::CratesIo,
        packages: vec!["example".to_owned()],
        authentication: Some(ReleaseAuthentication::BootstrapToken),
        recovery: Some(RecoveryRelease {
            package: "example".to_owned(),
            version: String::new(),
            source_sha: "a".repeat(40),
        }),
        ..RustReleaseConfig::default()
    };
    for (suffix_len, accepted) in [(140, true), (144, true), (145, false)] {
        release.recovery.as_mut().expect("bootstrap record").version =
            format!("1.2.3+{}", "a".repeat(suffix_len));
        assert_eq!(release.validate("fixture").is_ok(), accepted);
    }
    for version in [
        "0.1.1",
        "1.2.3-alpha.1",
        "1.2.3+build.42",
        "1.2.3-rc.1+build.42",
    ] {
        release.recovery.as_mut().expect("bootstrap record").version = version.to_owned();
        assert!(
            release.validate("fixture").is_ok(),
            "valid version: {version}"
        );
    }
    for version in ["01.2.3", "1.02.3", "1.2.3-01", "1.2.3+", "1.2", "1.2.3-"] {
        release.recovery.as_mut().expect("bootstrap record").version = version.to_owned();
        assert!(
            release.validate("fixture").is_err(),
            "invalid version: {version}"
        );
    }
}
