//! Reconstruction rejects alternate wires and unqualified captured evidence.

use super::super::{MODE, RustSourceDescriptor, RustSourceSelection};

fn fixture() -> RustSourceDescriptor {
    RustSourceDescriptor {
        schema: 1,
        rust_version: velnor_actions_mise::catalog::RUST_VERSION.to_owned(),
        target: velnor_actions_mise::catalog::RUST_TARGET_TRIPLE.to_owned(),
        roots: vec![String::new()],
        manifests: vec![(
            "Cargo.toml".to_owned(),
            "[package]\nname='demo'\nversion='1.0.0'\n[dependencies]\nz='1'\n".to_owned(),
        )],
        locks: vec![(
            String::new(),
            format!(
                "version=4\n[[package]]\nname='demo'\nversion='1.0.0'\n\
             [[package]]\nname='z'\nversion='1.0.0'\n\
             source='registry+https://github.com/rust-lang/crates.io-index'\n\
             checksum='{}'\n",
                "a".repeat(64)
            ),
        )],
        archives: vec![("z".to_owned(), "1.0.0".to_owned(), "a".repeat(64))],
        mode: MODE.to_owned(),
        selections: Vec::new(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn complete_and_selected_roundtrip_through_one_owner() {
    let mut descriptor = fixture();
    assert_eq!(
        RustSourceDescriptor::from_hex(&descriptor.hex_json().expect("wire")).expect("owner"),
        descriptor
    );
    descriptor.mode = "native-tree-selected-containing".to_owned();
    descriptor.selections.push(RustSourceSelection {
        root: String::new(),
        package: "demo".to_owned(),
        target: None,
        features: Vec::new(),
        default_features: true,
    });
    assert_eq!(
        RustSourceDescriptor::from_hex(&descriptor.hex_json().expect("wire")).expect("owner"),
        descriptor
    );
}

#[test]
fn noncanonical_unknown_duplicate_and_malformed_wires_are_rejected() {
    let descriptor = fixture();
    let json = String::from_utf8(descriptor.json().expect("wire")).expect("utf8");
    for changed in [
        format!(" {json}"),
        json.replacen('{', "{\"unknown\":1,", 1),
        json.replacen('{', "{\"schema\":1,", 1),
        json.replacen("\"schema\"", "\"\\u0073chema\"", 1),
        json.replacen(
            &format!(
                "\"schema\":1,\"rust_version\":\"{}\"",
                descriptor.rust_version
            ),
            &format!(
                "\"rust_version\":\"{}\",\"schema\":1",
                descriptor.rust_version
            ),
            1,
        ),
        json.replace("\"mode\":", "\"selections\":[],\"mode\":"),
        "{}".to_owned(),
    ] {
        assert!(RustSourceDescriptor::from_hex(&hex(changed.as_bytes())).is_err());
    }
    for wire in ["", "0", "ff", "AA", "zz"] {
        assert!(RustSourceDescriptor::from_hex(wire).is_err());
    }
}

#[test]
fn changed_pin_archive_sources_paths_and_mode_are_rejected() {
    let baseline = fixture();
    let mut mutations = Vec::new();
    let mut changed = baseline.clone();
    changed.rust_version = "1.97.0".to_owned();
    mutations.push(changed);
    let mut changed = baseline.clone();
    changed.archives[0].2 = "b".repeat(64);
    mutations.push(changed);
    let mut changed = baseline.clone();
    changed.locks[0].1 = changed.locks[0].1.replace(
        "registry+https://github.com/rust-lang/crates.io-index",
        "git+https://private",
    );
    mutations.push(changed);
    let mut changed = baseline.clone();
    changed.manifests[0].0 = "../Cargo.toml".to_owned();
    mutations.push(changed);
    let mut changed = baseline;
    changed.mode = "native-tree-selected-containing".to_owned();
    mutations.push(changed);
    for changed in mutations {
        assert!(RustSourceDescriptor::from_hex(&changed.hex_json().expect("wire")).is_err());
    }
}

#[test]
fn selected_policy_rejects_duplicates_unknown_fields_and_unknown_packages() {
    let mut descriptor = fixture();
    descriptor.mode = "native-tree-selected-containing".to_owned();
    descriptor.selections.push(RustSourceSelection {
        root: String::new(),
        package: "missing".to_owned(),
        target: None,
        features: Vec::new(),
        default_features: true,
    });
    assert!(RustSourceDescriptor::from_hex(&descriptor.hex_json().expect("wire")).is_err());
    descriptor.selections[0].package = "demo".to_owned();
    descriptor.selections.push(descriptor.selections[0].clone());
    assert!(RustSourceDescriptor::from_hex(&descriptor.hex_json().expect("wire")).is_err());
    descriptor.selections.pop();
    let json = String::from_utf8(descriptor.json().expect("wire")).expect("utf8");
    let wire = json.replace(
        "\"default_features\":",
        "\"unknown\":1,\"default_features\":",
    );
    assert!(RustSourceDescriptor::from_hex(&hex(wire.as_bytes())).is_err());
}
