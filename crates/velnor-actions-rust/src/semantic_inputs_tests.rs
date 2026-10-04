//! Static dependency and source-completeness regression cases.

use super::{
    SemanticInventory, dependencies, relative_path, source_completeness, validate_target_paths,
};
use std::collections::BTreeMap;
use std::path::Path;
use velnor_actions_contract::Provenance;

#[test]
fn every_local_dependency_class_is_reachable() {
    let manifest = toml::from_str::<toml::Value>(
        r#"
[dependencies]
ordinary = { path = "../ordinary" }
optional = { path = "../optional", optional = true }
[build-dependencies]
build = { path = "../build" }
[dev-dependencies]
dev = { path = "../dev" }
[target.'cfg(unix)'.dependencies]
target = { path = "../target" }
"#,
    )
    .expect("manifest");
    let mut paths = Vec::new();
    dependencies("crates/app/Cargo.toml", &manifest, None, &mut paths).expect("local dependencies");
    paths.sort();
    assert_eq!(
        paths,
        [
            "crates/build/Cargo.toml",
            "crates/dev/Cargo.toml",
            "crates/optional/Cargo.toml",
            "crates/ordinary/Cargo.toml",
            "crates/target/Cargo.toml"
        ]
    );
}

#[test]
fn inherited_dependency_uses_workspace_relative_path() {
    let member = toml::from_str::<toml::Value>(
        "[dependencies]\nshared = { workspace = true, optional = true }\n",
    )
    .expect("member");
    let workspace = (
        "Cargo.toml".to_owned(),
        toml::from_str::<toml::Value>(
            "[workspace.dependencies]\nshared = { path = 'crates/shared' }\n",
        )
        .expect("workspace"),
    );
    let mut paths = Vec::new();
    dependencies(
        "crates/app/Cargo.toml",
        &member,
        Some(&workspace),
        &mut paths,
    )
    .expect("inherited dependency");
    assert_eq!(paths, ["crates/shared/Cargo.toml"]);
}

#[test]
fn missing_or_opaque_dependency_is_unknown() {
    for text in [
        "[dependencies]\na = '1.0'\n",
        "[dependencies]\na = { workspace = true }\n",
        "[dependencies]\na = { git = 'https://example.invalid/a' }\n",
    ] {
        let manifest = toml::from_str::<toml::Value>(text).expect("manifest");
        assert!(dependencies("Cargo.toml", &manifest, None, &mut Vec::new()).is_err());
    }
}

#[test]
fn opaque_source_consumers_never_receive_complete_proof() {
    for text in [
        "include_str!(\"../../ignored.md\")",
        "include_bytes!(concat!(env!(\"OUT_DIR\"), \"/data\"))",
        "use std::fs::read as fetch; fn f() { fetch(\"data\"); }",
        "#[derive(Custom)] struct Item;",
        "#[path = \"../external.rs\"] mod external;",
        "macro_rules! opaque { () => {} }",
    ] {
        assert!(source_completeness("src/lib.rs", text.as_bytes()).is_err());
    }
    assert!(source_completeness("src/lib.rs", b"pub fn number() -> u8 { 3 }").is_ok());
}

#[test]
fn relative_paths_never_escape_the_checkout() {
    assert_eq!(
        relative_path("crates/a/Cargo.toml", "../b/Cargo.toml").expect("local"),
        "crates/b/Cargo.toml"
    );
    assert!(relative_path("Cargo.toml", "../outside/Cargo.toml").is_err());
    assert!(relative_path("Cargo.toml", "/outside/Cargo.toml").is_err());
}

#[test]
fn external_and_ignored_custom_target_paths_are_unknown() {
    let inventory = SemanticInventory {
        paths: vec!["external/source.md".to_owned()],
        provenance: Provenance::Known {
            digest: "verified".to_owned(),
        },
    };
    for source in ["../../external/source.md", "target/generated.rs"] {
        let manifest = toml::from_str::<toml::Value>(&format!("[lib]\npath = '{source}'\n"))
            .expect("manifest");
        assert!(
            validate_target_paths(
                Path::new("/nonexistent"),
                "crates/a/Cargo.toml",
                &manifest,
                &inventory,
                &mut BTreeMap::new()
            )
            .is_err()
        );
    }
}
