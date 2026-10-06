use super::*;

fn package(features: &[&str]) -> PackageRecord {
    PackageRecord {
        id: "demo@1.0.0".to_owned(),
        name: "demo".to_owned(),
        version: "1.0.0".to_owned(),
        manifest: "Cargo.toml".to_owned(),
        external: false,
        in_workspace: true,
        targets: Vec::new(),
        features: features.iter().map(|name| (*name).to_owned()).collect(),
        has_build_script: false,
    }
}

fn configuration(mode: RustFeatureMode, features: &[&str]) -> RustConfiguration {
    RustConfiguration {
        name: "coverage".to_owned(),
        features: features.iter().map(|name| (*name).to_owned()).collect(),
        feature_mode: mode,
        target: "host".to_owned(),
    }
}

#[test]
fn all_features_enable_default_and_every_actual_name() {
    let package = package(&["default", "all", "optional", "test-overrides"]);
    let config = configuration(RustFeatureMode::All, &[]);
    let (features, fallback) = resolve(&package, &config, &BTreeSet::new()).expect("all features");
    assert_eq!(features, ["all", "default", "optional", "test-overrides"]);
    assert!(fallback.is_none());
}

#[test]
fn all_features_change_when_the_manifest_adds_a_feature() {
    let config = configuration(RustFeatureMode::All, &[]);
    let before = resolve(&package(&["default"]), &config, &BTreeSet::new()).expect("before");
    let after = resolve(&package(&["default", "new"]), &config, &BTreeSet::new()).expect("after");
    assert_ne!(before.0, after.0);
    assert_eq!(after.0, ["default", "new"]);
}

#[test]
fn selected_feature_named_all_does_not_enable_other_features() {
    let package = package(&["all", "default", "other"]);
    let config = configuration(RustFeatureMode::Selected, &["all"]);
    let union = package.features.iter().cloned().collect();
    let (features, _) = resolve(&package, &config, &union).expect("selected all name");
    assert_eq!(features, ["all"]);
}

#[test]
fn default_and_selected_keeps_both_feature_sets() {
    let package = package(&["default", "extra"]);
    let config = configuration(RustFeatureMode::DefaultAndSelected, &["extra"]);
    let union = package.features.iter().cloned().collect();
    let (features, _) = resolve(&package, &config, &union).expect("default plus extra");
    assert_eq!(features, ["default", "extra"]);
}

#[test]
fn default_and_selected_without_declared_defaults_does_not_invent_a_feature() {
    let package = package(&["extra"]);
    let config = configuration(RustFeatureMode::DefaultAndSelected, &["extra"]);
    let union = package.features.iter().cloned().collect();
    let (features, _) = resolve(&package, &config, &union).expect("no default declaration");
    assert_eq!(features, ["extra"]);
}

#[test]
fn all_features_on_a_featureless_package_is_empty() {
    let config = configuration(RustFeatureMode::All, &[]);
    let (features, _) = resolve(&package(&[]), &config, &BTreeSet::new()).expect("featureless");
    assert!(features.is_empty());
}

#[test]
fn unknown_selected_features_still_fail_closed() {
    let config = configuration(RustFeatureMode::DefaultAndSelected, &["typo"]);
    let err = resolve(&package(&["default"]), &config, &BTreeSet::new()).expect_err("typo");
    assert!(err.to_string().contains("unknown_feature"));
}

#[test]
fn fallback_reports_defaults_added_by_the_selected_policy() {
    let package = package(&["default", "extra"]);
    let config = configuration(RustFeatureMode::DefaultAndSelected, &["extra", "elsewhere"]);
    let union = ["default", "extra", "elsewhere"]
        .map(str::to_owned)
        .into_iter()
        .collect();
    let (features, fallback) = resolve(&package, &config, &union).expect("partial feature set");
    assert_eq!(features, ["default", "extra"]);
    assert_eq!(fallback.expect("record narrowing").applied, features);
}
