// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::image_deploy::release_version::{Release, check};

fn package(name: &str, version: &str, targets: &[(&str, &str)]) -> Value {
    json!({
        "name": name,
        "version": version,
        "targets": targets
            .iter()
            .map(|(target, kind)| json!({"name": target, "kind": [kind]}))
            .collect::<Vec<_>>(),
    })
}

fn metadata(packages: &[Value]) -> Vec<u8> {
    json!({"packages": packages, "workspace_members": []})
        .to_string()
        .into_bytes()
}

fn bins(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn run(
    packages: &[Value],
    version: &str,
    names: &[&str],
    package: Option<&str>,
) -> Result<String, String> {
    check(
        &metadata(packages),
        &Release {
            version,
            bins: &bins(names),
            package,
        },
    )
    .map_err(|error| error.to_string())
}

fn single() -> Vec<Value> {
    vec![package("svc", "1.2.3", &[("svc", "bin")])]
}

fn workspace() -> Vec<Value> {
    vec![
        package("core", "9.9.9", &[("core", "lib")]),
        package("svc", "1.2.3", &[("svc", "bin"), ("svc", "lib")]),
        package("other", "4.0.0", &[("other", "bin")]),
    ]
}

#[test]
fn a_tag_that_names_the_version_of_the_crate_that_builds_the_bins_passes() {
    let report = run(&single(), "1.2.3", &["svc"], None).unwrap_or_default();
    assert!(
        report.contains("svc") && report.contains("1.2.3"),
        "{report}"
    );
}

#[test]
fn a_tag_with_another_version_is_refused_naming_both_versions_and_the_package() {
    let message = run(&single(), "1.2.4", &["svc"], None)
        .err()
        .unwrap_or_default();
    assert!(
        message.contains("1.2.4") && message.contains("1.2.3") && message.contains("svc"),
        "{message}"
    );
    assert!(message.contains("nothing was promoted"), "{message}");
}

#[test]
fn a_workspace_resolves_the_package_that_owns_the_bins_and_not_its_neighbours() {
    assert!(run(&workspace(), "1.2.3", &["svc"], None).is_ok());
    for neighbour in ["9.9.9", "4.0.0"] {
        let message = run(&workspace(), neighbour, &["svc"], None)
            .err()
            .unwrap_or_default();
        assert!(
            message.contains(neighbour) && message.contains("1.2.3") && message.contains("svc"),
            "{message}"
        );
    }
    assert!(run(&workspace(), "4.0.0", &["other"], None).is_ok());
}

#[test]
fn a_library_target_of_the_same_name_does_not_own_a_bin() {
    let packages = vec![
        package("svc-lib", "0.1.0", &[("svc", "lib")]),
        package("svc", "1.2.3", &[("svc", "bin")]),
    ];
    assert!(run(&packages, "1.2.3", &["svc"], None).is_ok());
    assert!(run(&packages, "0.1.0", &["svc"], None).is_err());
}

#[test]
fn a_bin_that_two_packages_build_needs_the_package_to_choose_between_them() {
    let packages = vec![
        package("first", "1.0.0", &[("tool", "bin")]),
        package("second", "2.0.0", &[("tool", "bin")]),
    ];
    let message = run(&packages, "1.0.0", &["tool"], None)
        .err()
        .unwrap_or_default();
    assert!(
        message.contains("first") && message.contains("second") && message.contains("package"),
        "{message}"
    );
    assert!(run(&packages, "2.0.0", &["tool"], Some("second")).is_ok());
    assert!(run(&packages, "1.0.0", &["tool"], Some("second")).is_err());
}

#[test]
fn bins_that_no_package_builds_or_that_sit_in_several_packages_are_refused() {
    let message = run(&workspace(), "1.2.3", &["missing"], None)
        .err()
        .unwrap_or_default();
    assert!(message.contains("missing"), "{message}");
    let split = run(&workspace(), "1.2.3", &["svc", "other"], None)
        .err()
        .unwrap_or_default();
    assert!(split.contains("svc") && split.contains("other"), "{split}");
    let absent = run(&workspace(), "1.2.3", &["svc"], Some("nope"))
        .err()
        .unwrap_or_default();
    assert!(absent.contains("nope"), "{absent}");
}

#[test]
fn a_tag_without_a_plain_version_is_refused_the_way_promote_refuses_it() {
    for version in ["latest", "1.2", "1.2.3.4", "v1.2.3", "1.2.3-rc.1"] {
        let message = run(&single(), version, &["svc"], None)
            .err()
            .unwrap_or_default();
        assert!(
            message.contains(&format!("`{version}` is not a version X.Y.Z")),
            "{version}: {message}"
        );
    }
}

#[test]
fn output_that_is_not_cargo_metadata_is_refused() {
    let release = Release {
        version: "1.2.3",
        bins: &bins(&["svc"]),
        package: None,
    };
    let message = check(b"not json", &release)
        .err()
        .map_or_else(String::new, |error| error.to_string());
    assert!(message.contains("cargo metadata"), "{message}");
}
