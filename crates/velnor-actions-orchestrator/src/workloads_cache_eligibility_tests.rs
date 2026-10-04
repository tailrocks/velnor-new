use super::*;
use velnor_actions_contract::build_index;

fn fixture(resolved: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("fixture");
    std::fs::write(
        temp.path().join("package.json"),
        r#"{"scripts":{"typecheck":"tsc"},"devDependencies":{"typescript":"^5.0.0"}}"#,
    )
    .expect("manifest");
    std::fs::write(
        temp.path().join("package-lock.json"),
        serde_json::json!({
            "lockfileVersion": 3, "packages": {
                "": {"devDependencies": {"typescript": "^5.0.0"}},
                "node_modules/typescript": {"version": "5.0.0", "resolved": resolved,
                    "integrity": format!("sha512-{}==", "A".repeat(86))}
            }
        })
        .to_string(),
    )
    .expect("lock");
    temp
}

fn qualified(temp: &tempfile::TempDir, kind: &str, root: &str) -> bool {
    let index = build_index(temp.path(), &[]).expect("index");
    kind == "node_ci" && source_candidates(&index, root).is_some()
}

#[test]
fn declared_locked_candidates_preserve_project_scripts() {
    let temp = fixture("https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz");
    assert!(qualified(&temp, "node_ci", "."));
    assert!(!qualified(&temp, "bun_ci", "."));
    assert!(!qualified(&temp, "gradle_test", "."));
}

#[test]
fn custom_git_local_token_and_ambiguous_urls_fail_closed() {
    for url in [
        "https://private.example/typescript.tgz",
        "git+https://github.com/example/private.git",
        "file:private.tgz",
        "https://token@registry.npmjs.org/typescript/-/typescript.tgz",
        "https://registry.npmjs.org.evil.test/typescript/-/typescript.tgz",
        "https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz?token=private",
        "https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz#private",
        "https://registry.npmjs.org/../private/-/typescript.tgz",
        "https://registry.npmjs.org/typescript%2fprivate/-/typescript.tgz",
    ] {
        assert!(!qualified(&fixture(url), "node_ci", "."), "{url}");
    }
}

#[test]
fn config_and_unknown_or_unreadable_inputs_disable_only_transport() {
    for config in [".npmrc", "bunfig.toml", "npm-shrinkwrap.json"] {
        let temp = fixture("https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz");
        std::fs::write(temp.path().join(config), "registry=https://private.example")
            .expect("source config");
        assert!(!qualified(&temp, "node_ci", "."));
    }
    let temp = fixture("https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz");
    std::fs::write(temp.path().join("package-lock.json"), "malformed").expect("malformed");
    assert!(!qualified(&temp, "node_ci", "."));
    assert!(!qualified(&temp, "node_ci", "missing"));
    assert!(!qualified(&temp, "node_ci", ".."));
}

#[test]
fn source_configs_cannot_hide_in_index_exclusions() {
    let temp = fixture("https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz");
    std::fs::write(temp.path().join(".npmrc"), "").expect("npm config");
    let index = build_index(temp.path(), &[".npmrc".to_owned()]).expect("index");
    assert!(source_candidates(&index, ".").is_none());
}

#[test]
fn legacy_lock_graph_cannot_hide_private_sources() {
    let temp = fixture("https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz");
    let path = temp.path().join("package-lock.json");
    let mut lock: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("lock")).expect("JSON lock");
    lock["lockfileVersion"] = Value::from(2);
    lock["dependencies"] = serde_json::json!({"private": {
        "resolved": "https://private.example/package.tgz"
    }});
    std::fs::write(path, lock.to_string()).expect("legacy graph");
    assert!(!qualified(&temp, "node_ci", "."));
}

#[test]
fn unsupported_graphs_and_lifecycle_downloads_fail_closed() {
    for packages in [
        serde_json::json!({"": {}, "node_modules/demo": {"link": true, "resolved": "workspace"}}),
        serde_json::json!({"": {}, "node_modules/demo": {"version": "1.0.0"}}),
        serde_json::json!({"": {}, "node_modules/demo": {
            "resolved": "https://registry.npmjs.org/demo/-/demo.tgz", "hasInstallScript": true}}),
        serde_json::json!({"": {"workspaces": ["private"]}}),
    ] {
        let temp = fixture("https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz");
        std::fs::write(
            temp.path().join("package-lock.json"),
            serde_json::json!({
                "lockfileVersion": 3, "packages": packages
            })
            .to_string(),
        )
        .expect("unsupported graph");
        assert!(!qualified(&temp, "node_ci", "."));
    }
}

#[test]
fn unlocked_manifest_source_overrides_and_parent_config_fail_closed() {
    let temp = fixture("https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz");
    std::fs::write(
        temp.path().join("package.json"),
        r#"{"dependencies":{"private":"git+https://github.com/example/private.git"}}"#,
    )
    .expect("private manifest");
    assert!(!qualified(&temp, "node_ci", "."));
    let temp = fixture("https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz");
    std::fs::create_dir(temp.path().join("nested")).expect("nested");
    for file in ["package.json", "package-lock.json"] {
        std::fs::copy(
            temp.path().join(file),
            temp.path().join("nested").join(file),
        )
        .expect("nested input");
    }
    assert!(qualified(&temp, "node_ci", "nested"));
    std::fs::write(temp.path().join(".npmrc"), "").expect("parent config");
    assert!(!qualified(&temp, "node_ci", "nested"));
}

#[test]
fn integrities_are_exact_canonical_sha512_and_hooks_remain_unqualified() {
    assert!(sha512_integrity(&format!("sha512-{}==", "A".repeat(86))));
    for integrity in [
        "sha512-short==".to_owned(),
        format!("sha256-{}==", "A".repeat(86)),
        format!("sha512-{}B==", "A".repeat(85)),
        format!("sha512-{}== sha512-private", "A".repeat(86)),
    ] {
        assert!(!sha512_integrity(&integrity));
    }
    for hook in [
        "preinstall",
        "install",
        "postinstall",
        "prepare",
        "prepublishOnly",
    ] {
        let temp = fixture("https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz");
        std::fs::write(
            temp.path().join("package.json"),
            serde_json::json!({
                "scripts": {hook: "fetch-private-data"}
            })
            .to_string(),
        )
        .expect("hook");
        assert!(!qualified(&temp, "node_ci", "."));
    }
}

#[test]
fn scoped_and_nested_v3_sources_have_exact_immutable_descriptors() {
    let temp = fixture("https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz");
    let integrity = format!("sha512-{}==", "A".repeat(86));
    std::fs::write(
        temp.path().join("package-lock.json"),
        serde_json::json!({
            "lockfileVersion": 3, "packages": {
                "": {},
                "node_modules/@types/node": {
                    "version": "22.0.0", "integrity": integrity,
                    "resolved": "https://registry.npmjs.org/@types/node/-/node-22.0.0.tgz"
                },
                "node_modules/parent/node_modules/@types/node": {
                    "version": "20.0.0", "integrity": integrity,
                    "resolved": "https://registry.npmjs.org/@types/node/-/node-20.0.0.tgz"
                },
                "node_modules/@scope/parent/node_modules/child": {
                    "version": "1.2.3-beta.0+build.1", "integrity": integrity,
                    "resolved": "https://registry.npmjs.org/child/-/child-1.2.3-beta.0+build.1.tgz"
                }
            }
        })
        .to_string(),
    )
    .expect("scoped lock");
    let index = build_index(temp.path(), &[]).expect("index");
    let sources = source_candidates(&index, ".").expect("source descriptors");
    assert_eq!(sources.len(), 3);
    assert_eq!(sources[0].name, "@types/node");
    assert_eq!(sources[0].version, "20.0.0");
    assert_eq!(sources[2].name, "child");
    let serialized = serde_json::to_string(&sources).expect("descriptor JSON");
    assert_eq!(
        serde_json::from_str::<Vec<NativeNpmSource>>(&serialized)
            .expect("typed descriptor roundtrip"),
        sources
    );
}

#[test]
fn ambiguous_package_names_aliases_and_mutable_versions_are_rejected() {
    for path in [
        "node_modules/../secret",
        "node_modules/@scope",
        "packages/demo",
        "node_modules/demo/unknown/node_modules/private",
        "node_modules/node_modules",
    ] {
        assert!(package_name(path).is_none(), "{path}");
    }
    for version in [
        "latest",
        "^1.2.3",
        "1.2",
        "01.2.3",
        "1.2.3-01",
        "1.2.3+",
        "1.2.3+foo+bar",
        "1.2.3-",
        "1.2.3-a..b",
    ] {
        assert!(!exact_version(version), "{version}");
    }
    let temp = fixture("https://registry.npmjs.org/typescript/-/other-5.0.0.tgz");
    assert!(!qualified(&temp, "node_ci", "."));
    let temp = fixture("https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz");
    let path = temp.path().join("package-lock.json");
    let mut lock: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("lock")).expect("JSON lock");
    lock["packages"]["node_modules/typescript"]["name"] = Value::from("private-alias");
    std::fs::write(path, lock.to_string()).expect("alias");
    assert!(!qualified(&temp, "node_ci", "."));
}

#[test]
fn oversized_source_closures_disable_optional_cache_before_bridge_admission() {
    let temp = fixture("https://registry.npmjs.org/typescript/-/typescript-5.0.0.tgz");
    let integrity = format!("sha512-{}==", "A".repeat(86));
    let mut packages = serde_json::Map::new();
    packages.insert(String::new(), serde_json::json!({}));
    for n in 0..=MAX_NPM_SOURCES {
        packages.insert(
            format!("node_modules/pkg{n}"),
            serde_json::json!({
                "version": "1.0.0", "integrity": integrity,
                "resolved": format!("https://registry.npmjs.org/pkg{n}/-/pkg{n}-1.0.0.tgz")
            }),
        );
    }
    let path = temp.path().join("package-lock.json");
    let write_lock = |packages: &serde_json::Map<String, Value>| {
        std::fs::write(
            &path,
            serde_json::json!({
                "lockfileVersion": 3, "packages": packages
            })
            .to_string(),
        )
        .expect("bounded source closure");
    };
    write_lock(&packages);
    let index = build_index(temp.path(), &[]).expect("index");
    assert!(source_candidates(&index, ".").is_none());
    packages.remove(&format!("node_modules/pkg{MAX_NPM_SOURCES}"));
    write_lock(&packages);
    assert_eq!(
        source_candidates(&index, ".")
            .expect("bounded closure")
            .len(),
        MAX_NPM_SOURCES
    );
}
