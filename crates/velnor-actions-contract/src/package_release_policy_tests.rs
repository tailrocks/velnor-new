use super::*;
use crate::package_release::PackageReleasePayloadFormat;
pub(crate) fn fixture() -> PackageReleasePolicy {
    PackageReleasePolicy {
        schema: PACKAGE_RELEASE_POLICY_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "example/tool".to_owned(),
        default_branch: "main".to_owned(),
        version: CargoVersionPolicy {
            version_package: "tool".to_owned(),
            bindings: vec![CargoVersionBinding {
                package: "tool".to_owned(),
                manifest_path: "crates/tool/Cargo.toml".to_owned(),
                workspace_manifest_path: "Cargo.toml".to_owned(),
            }],
        },
        products: vec![PackageReleasePlanProduct {
            id: "tool".to_owned(),
            package: "tool".to_owned(),
            bins: vec!["cli".to_owned()],
            targets: vec!["aarch64-apple-darwin".to_owned()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: vec![],
    }
}
#[test]
fn policy_has_no_version_value_and_plan_resolution_preserves_inventory() {
    let policy = fixture();
    let bytes = policy.canonical_bytes().expect("policy");
    assert_eq!(
        parse_package_release_policy(&bytes).expect("canonical"),
        policy
    );
    assert!(
        !String::from_utf8(bytes)
            .expect("utf8")
            .contains("base_version")
    );
    assert_eq!(
        policy.manifest_paths(),
        ["Cargo.toml", "crates/tool/Cargo.toml"]
    );
    for version in ["1.2.3", "2.0.0"] {
        let plan = policy.resolve(version.to_owned()).expect("resolved");
        policy.validate_plan(&plan).expect("same static policy");
        assert_eq!(plan.base_version, version);
    }
    assert!(policy.resolve("1.2.3-preview.1".to_owned()).is_err());
    let mut changed = policy.resolve("1.2.3".to_owned()).expect("plan");
    changed.products[0].bins[0] = "other".to_owned();
    assert!(policy.validate_plan(&changed).is_err());
}
#[test]
fn policy_rejects_incomplete_or_escaping_manifest_ownership() {
    let mut policy = fixture();
    policy.version.version_package = "absent".to_owned();
    assert!(policy.validate().is_err());
    let mut policy = fixture();
    policy.version.bindings.clear();
    assert!(policy.validate().is_err());
    for path in [
        "../Cargo.toml",
        "/Cargo.toml",
        "a//Cargo.toml",
        "./Cargo.toml",
        "Cargo.toml\n",
        "a\\Cargo.toml",
    ] {
        let mut policy = fixture();
        policy.version.bindings[0].manifest_path = path.to_owned();
        assert!(policy.validate().is_err(), "{path}");
    }
    let mut policy = fixture();
    policy
        .version
        .bindings
        .push(policy.version.bindings[0].clone());
    assert!(policy.validate().is_err());
}
#[test]
fn strict_policy_parser_rejects_version_literals_unknown_keys_and_duplicates() {
    let bytes = fixture().canonical_bytes().expect("policy");
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    value["base_version"] = "1.2.3".into();
    assert!(
        parse_package_release_policy(&crate::canonical_json_bytes(&value).expect("canonical"))
            .is_err()
    );
    let text = String::from_utf8(bytes).expect("utf8");
    assert!(parse_package_release_policy(format!(" {text}").as_bytes()).is_err());
    assert!(
        parse_package_release_policy(
            text.replacen('{', "{\"schema\":\"duplicate\",", 1)
                .as_bytes()
        )
        .is_err()
    );
}
