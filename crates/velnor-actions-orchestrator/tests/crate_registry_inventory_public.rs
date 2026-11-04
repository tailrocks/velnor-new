// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_orchestrator::{
    CrateInventoryError, OrchestratorError,
    crate_release_registry::{CrateRegistryRead, CrateRegistryResponse},
    inspect_crate_versions,
};

struct Reply(u16, Vec<u8>);
impl CrateRegistryRead for Reply {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        assert_eq!(path, "/api/v1/crates/demo");
        Ok(CrateRegistryResponse {
            status: self.0,
            body: self.1.clone(),
        })
    }
}

fn reply(body: &serde_json::Value) -> Reply {
    Reply(200, body.to_string().into_bytes())
}
fn good() -> serde_json::Value {
    serde_json::json!({"crate":{"id":"demo","name":"demo","versions":[1,2]},"versions":[{"id":1,"crate":"demo","num":"0.1.0","yanked":false},{"id":2,"crate":"demo","num":"0.2.0+old","yanked":true}]})
}
#[test]
fn registry_evidence_binds_all_versions_and_filters_yanked_for_resolution() {
    let result = inspect_crate_versions(&reply(&good()), "demo").expect("valid registry fixture");
    assert_eq!(result.registry, "crates-io");
    assert_eq!(result.package, "demo");
    assert_eq!(
        result.available_versions().into_iter().collect::<Vec<_>>(),
        ["0.1.0"]
    );
    assert!(result.versions["0.2.0+old"].yanked);
    assert!(result.has_build_collision("0.2.0+new"));
    assert_eq!(result.response_sha256.len(), 64);
    let mut cases = Vec::new();
    let mut missing = good();
    missing["crate"]["versions"] = serde_json::json!([1, 2, 3]);
    cases.push(missing);
    let mut duplicate = good();
    duplicate["versions"][1]["id"] = serde_json::json!(1);
    cases.push(duplicate);
    let mut foreign = good();
    foreign["versions"][0]["crate"] = serde_json::json!("other");
    cases.push(foreign);
    let mut other = good();
    other["crate"]["name"] = serde_json::json!("other");
    cases.push(other);
    let mut bad_version = good();
    bad_version["versions"][0]["num"] = serde_json::json!("not-semver");
    cases.push(bad_version);
    let mut bad_yank = good();
    bad_yank["versions"][0]["yanked"] = serde_json::Value::Null;
    cases.push(bad_yank);
    for value in cases {
        assert_eq!(
            inspect_crate_versions(&reply(&value), "demo"),
            Err(CrateInventoryError::InvalidResponse)
        );
    }
    let duplicate_key = br#"{"crate":{"id":"demo","id":"other","name":"demo","versions":[1]},"versions":[{"id":1,"crate":"demo","num":"0.1.0","yanked":false}]}"#;
    assert_eq!(
        inspect_crate_versions(&Reply(200, duplicate_key.to_vec()), "demo"),
        Err(CrateInventoryError::InvalidResponse)
    );
    assert_eq!(
        inspect_crate_versions(&Reply(200, vec![b' '; 8 * 1024 * 1024 + 1]), "demo"),
        Err(CrateInventoryError::InvalidResponse)
    );
}
#[test]
fn registry_absence_is_distinct_from_denial_throttling_and_server_failure() {
    for (status, expected) in [
        (404, CrateInventoryError::NotFound),
        (403, CrateInventoryError::Forbidden),
        (429, CrateInventoryError::RateLimited),
        (500, CrateInventoryError::ServerError),
        (302, CrateInventoryError::UnexpectedStatus),
    ] {
        assert_eq!(
            inspect_crate_versions(&Reply(status, vec![]), "demo"),
            Err(expected)
        );
    }
}

#[test]
fn yanked_registry_versions_do_not_satisfy_real_cargo_dependency_graph()
-> Result<(), Box<dyn std::error::Error>> {
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs,
        process::Command,
    };
    use velnor_actions_rust::{
        release_graph::{RegistryPackage, RegistryState, plan_release},
        release_select::{ReleaseRequest, ReleaseScope},
    };
    let temp = tempfile::tempdir()?;
    fs::create_dir(temp.path().join("src"))?;
    fs::write(temp.path().join("src/lib.rs"), "pub fn f() {}")?;
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\ndemo = \"=0.2.0\"\n",
    )?;
    let output = Command::new(env!("CARGO"))
        .current_dir(temp.path())
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .output()?;
    assert!(output.status.success());
    let json = String::from_utf8(output.stdout)?;
    let scope = ReleaseScope::Packages(vec!["app".into()]);
    let affected = BTreeSet::new();
    let manifests = BTreeSet::from(["Cargo.toml".into()]);
    let canonical = temp.path().canonicalize()?;
    let request = ReleaseRequest {
        metadata_json: &json,
        repo_root: &canonical,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &affected,
        known_manifests: &manifests,
        supported_registries: &[],
    };
    let inventory =
        inspect_crate_versions(&reply(&good()), "demo").expect("valid registry fixture");
    let key = RegistryPackage {
        registry: inventory.registry.clone(),
        package: inventory.package.clone(),
    };
    let mut state = RegistryState {
        published: BTreeMap::from([(key.clone(), inventory.available_versions())]),
    };
    assert!(plan_release(&request, &state).is_err());
    let mut unyanked = good();
    unyanked["versions"][1]["yanked"] = serde_json::json!(false);
    state.published.insert(
        key,
        inspect_crate_versions(&reply(&unyanked), "demo")
            .expect("valid registry fixture")
            .available_versions(),
    );
    plan_release(&request, &state).expect("non-yanked dependency is available");
    Ok(())
}

#[test]
fn registry_snapshot_ignores_download_counts_but_tracks_versions_and_yanks() {
    let first = good();
    let mut changed = good();
    changed["crate"]["downloads"] = serde_json::json!(123);
    let original = inspect_crate_versions(&reply(&first), "demo").expect("valid original");
    let counters = inspect_crate_versions(&reply(&changed), "demo").expect("valid counters");
    assert_ne!(original.response_sha256, counters.response_sha256);
    assert_eq!(original.snapshot_sha256(), counters.snapshot_sha256());
    changed["versions"][1]["yanked"] = serde_json::json!(false);
    let changed_yank =
        inspect_crate_versions(&reply(&changed), "demo").expect("valid changed yank");
    assert_ne!(original.snapshot_sha256(), changed_yank.snapshot_sha256());
    let mut added = good();
    added["crate"]["versions"] = serde_json::json!([1, 2, 3]);
    added["versions"]
        .as_array_mut()
        .expect("versions")
        .push(serde_json::json!({"id":3,"crate":"demo","num":"0.3.0","yanked":false}));
    assert_ne!(
        original.snapshot_sha256(),
        inspect_crate_versions(&reply(&added), "demo")
            .expect("valid added version")
            .snapshot_sha256()
    );
    let mut replaced_id = good();
    replaced_id["crate"]["versions"] = serde_json::json!([1, 4]);
    replaced_id["versions"][1]["id"] = serde_json::json!(4);
    assert_ne!(
        original.snapshot_sha256(),
        inspect_crate_versions(&reply(&replaced_id), "demo")
            .expect("valid replaced ID")
            .snapshot_sha256()
    );
}
