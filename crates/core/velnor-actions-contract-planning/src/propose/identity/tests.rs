use super::*;

/// Component mapping keeps every documented case byte-identical.
#[test]
fn component_ids_shed_checkout_paths() {
    assert_eq!(component_id_for_unit("demo", "Cargo.toml"), "demo");
    assert_eq!(
        component_id_for_unit("demo 0.1.0", "Cargo.toml"),
        "demo 0.1.0"
    );
    assert_eq!(
        component_id_for_unit("path+file:///Users/dev/proj#demo@0.1.0", "Cargo.toml"),
        "demo@0.1.0"
    );
    assert_eq!(
        component_id_for_unit(
            "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0",
            "Cargo.toml"
        ),
        "serde@1.0.0"
    );
    assert_eq!(component_id_for_unit("", "Cargo.toml"), "workspace");
    assert_eq!(component_id_for_unit("", "crates/a/Cargo.toml"), "crates/a");
}

/// Project roots anchor manifests; the repository root is `.`.
#[test]
fn project_roots_anchor_unit_paths() {
    assert_eq!(project_root_for_unit_path("Cargo.toml"), ".");
    assert_eq!(
        project_root_for_unit_path("crates/a/Cargo.toml"),
        "crates/a"
    );
}

/// Validation pins shapes without owning adapter spellings.
#[test]
fn validation_pins_shapes_not_spellings() {
    let valid = IdentityInputs {
        unit_id: "demo".to_owned(),
        unit_key: "root".to_owned(),
        unit_path: "Cargo.toml".to_owned(),
        project_root: ".".to_owned(),
        target: "host".to_owned(),
        features: vec!["a".to_owned(), "b".to_owned()],
        flags: Vec::new(),
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_nextest".to_owned(),
        environment: BTreeMap::new(),
        declared_inputs: Vec::new(),
        undeclared_reads: false,
    };
    assert!(valid.validate().is_ok());
    let unordered = IdentityInputs {
        features: vec!["b".to_owned(), "a".to_owned()],
        ..valid.clone()
    };
    assert!(unordered.validate().is_err());
    let bad_key = IdentityInputs {
        unit_key: String::new(),
        ..valid.clone()
    };
    assert!(bad_key.validate().is_err());
}
