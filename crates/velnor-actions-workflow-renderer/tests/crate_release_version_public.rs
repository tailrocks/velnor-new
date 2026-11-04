// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_workflow_renderer::release_spec::validate_package_version;

#[test]
fn release_workflow_versions_follow_cargo_semver() {
    let long_version = format!("1.2.3+{}", "a".repeat(140));
    assert!(validate_package_version(&long_version).is_ok());
    let max_version = format!("1.2.3+{}", "a".repeat(144));
    assert!(validate_package_version(&max_version).is_ok());
    let too_long = format!("1.2.3+{}", "a".repeat(145));
    assert!(validate_package_version(&too_long).is_err());
    for version in [
        "0.1.1",
        "1.2.3-alpha.1",
        "1.2.3+build.42",
        "1.2.3-rc.1+build.42",
    ] {
        assert!(
            validate_package_version(version).is_ok(),
            "valid version: {version}"
        );
    }
    for version in ["01.2.3", "1.02.3", "1.2.3-01", "1.2.3+", "1.2", "1.2.3-"] {
        assert!(
            validate_package_version(version).is_err(),
            "invalid version: {version}"
        );
    }
}
