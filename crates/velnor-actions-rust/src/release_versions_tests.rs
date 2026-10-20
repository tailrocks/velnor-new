use super::*;
use velnor_actions_contract::package_release::{
    PackageReleaseChannel, PackageReleasePayloadFormat,
};
use velnor_actions_contract::{
    CargoVersionBinding, CargoVersionPolicy, PACKAGE_RELEASE_POLICY_SCHEMA_V1,
    PackageReleasePlanProduct,
};
fn fixture() -> (PackageReleasePolicy, BTreeMap<String, Vec<u8>>) {
    let products = ["capsule", "scanner"]
        .into_iter()
        .map(|package| PackageReleasePlanProduct {
            id: package.to_owned(),
            package: package.to_owned(),
            bins: vec![format!("{package}-cli")],
            targets: vec!["aarch64-apple-darwin".to_owned()],
            format: PackageReleasePayloadFormat::TarGz,
        })
        .collect();
    let policy = PackageReleasePolicy {
        schema: PACKAGE_RELEASE_POLICY_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "example/scanner".to_owned(),
        default_branch: "main".to_owned(),
        version: CargoVersionPolicy {
            version_package: "scanner".to_owned(),
            bindings: ["capsule", "scanner"]
                .into_iter()
                .map(|package| CargoVersionBinding {
                    package: package.to_owned(),
                    manifest_path: format!("crates/{package}/Cargo.toml"),
                    workspace_manifest_path: "Cargo.toml".to_owned(),
                })
                .collect(),
        },
        products,
        companions: vec![],
    };
    let manifests = BTreeMap::from([
        ("Cargo.toml".to_owned(), b"[workspace]\nmembers=['crates/capsule','crates/scanner']\n[workspace.package]\nversion='1.2.3'\n".to_vec()),
        ("crates/capsule/Cargo.toml".to_owned(), b"[package]\nname='capsule'\nversion='1.2.3'\n".to_vec()),
        ("crates/scanner/Cargo.toml".to_owned(), b"[package]\nname='scanner'\nversion.workspace=true\nworkspace='../..'\n".to_vec()),
    ]);
    (policy, manifests)
}
#[test]
fn version_only_changes_resolve_new_plan_without_generated_policy_churn() {
    let (policy, mut manifests) = fixture();
    let before = policy.canonical_bytes().expect("policy");
    let old = resolve_release_policy(&policy, &manifests).expect("inline and inherited");
    for path in ["Cargo.toml", "crates/capsule/Cargo.toml"] {
        let text = String::from_utf8(manifests[path].clone())
            .expect("utf8")
            .replace("1.2.3", "2.0.0");
        manifests.insert(path.to_owned(), text.into_bytes());
    }
    let new = resolve_release_policy(&policy, &manifests).expect("new version data");
    assert_eq!(before, policy.canonical_bytes().expect("same policy"));
    assert_eq!(old.base_version, "1.2.3");
    assert_eq!(new.base_version, "2.0.0");
    policy.validate_plan(&old).expect("old static inventory");
    policy.validate_plan(&new).expect("new static inventory");
    assert_eq!(old.products, new.products);
    let mut stable = policy;
    stable.channel = PackageReleaseChannel::Stable;
    assert_eq!(
        resolve_release_policy(&stable, &manifests)
            .expect("same stable source")
            .base_version,
        "2.0.0"
    );
}
#[test]
fn resolution_requires_exact_package_names_and_equal_numeric_product_versions() {
    let (policy, manifests) = fixture();
    for version in ["1.2.4", "1.2.3-preview.1", "v1.2.3", "01.2.3"] {
        let mut input = manifests.clone();
        input.insert(
            "crates/capsule/Cargo.toml".to_owned(),
            format!("[package]\nname='capsule'\nversion='{version}'\n").into_bytes(),
        );
        assert!(
            resolve_release_policy(&policy, &input).is_err(),
            "{version}"
        );
    }
    let mut input = manifests;
    input.insert(
        "crates/scanner/Cargo.toml".to_owned(),
        b"[package]\nname='other'\nversion.workspace=true\n".to_vec(),
    );
    assert!(resolve_release_policy(&policy, &input).is_err());
}
#[test]
fn resolution_rejects_missing_extra_oversized_or_ambiguous_manifest_data() {
    let (policy, manifests) = fixture();
    let mut input = manifests.clone();
    input.remove("Cargo.toml");
    assert!(resolve_release_policy(&policy, &input).is_err());
    let mut input = manifests.clone();
    input.insert(
        "unowned/Cargo.toml".to_owned(),
        b"[package]\nname='unowned'\nversion='1.2.3'".to_vec(),
    );
    assert!(resolve_release_policy(&policy, &input).is_err());
    for bytes in [
        vec![0xff],
        vec![b'x'; MAX_CARGO_VERSION_MANIFEST_BYTES + 1],
        b"[package]\nname='scanner'\nversion='1.2.3'\nversion='1.2.3'".to_vec(),
        b"[package]\nname='scanner'\nversion={workspace=true,other=true}".to_vec(),
        b"[package]\nname='scanner'\nversion={workspace=false}".to_vec(),
    ] {
        let mut input = manifests.clone();
        input.insert("crates/scanner/Cargo.toml".to_owned(), bytes);
        assert!(resolve_release_policy(&policy, &input).is_err());
    }
}
#[test]
fn inherited_version_uses_only_the_captured_workspace_owner() {
    let (policy, manifests) = fixture();
    for workspace in ["../../other", "../../..", "/tmp/other"] {
        let mut input = manifests.clone();
        input.insert(
            "crates/scanner/Cargo.toml".to_owned(),
            format!("[package]\nname='scanner'\nworkspace='{workspace}'\nversion.workspace=true\n")
                .into_bytes(),
        );
        assert!(
            resolve_release_policy(&policy, &input).is_err(),
            "{workspace}"
        );
    }
    let mut input = manifests;
    input.insert(
        "Cargo.toml".to_owned(),
        b"[workspace.package]\nversion={workspace=true}\n".to_vec(),
    );
    assert!(resolve_release_policy(&policy, &input).is_err());
}
