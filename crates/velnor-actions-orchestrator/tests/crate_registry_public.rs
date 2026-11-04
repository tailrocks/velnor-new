// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use velnor_actions_orchestrator::{
    OrchestratorError,
    crate_release_registry::{
        CrateRegistryRead, CrateRegistryResponse, inspect_crate_registry, reconcile_crate_registry,
    },
};

struct Registry(BTreeMap<String, (u16, Vec<u8>)>);
impl CrateRegistryRead for Registry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        let (status, body) = self.0.get(path).cloned().unwrap_or((500, Vec::new()));
        Ok(CrateRegistryResponse { status, body })
    }
}

#[test]
fn preflight_rejects_build_metadata_collision_and_incomplete_registry_list() {
    let make = |versions: serde_json::Value| {
        Registry(BTreeMap::from([
            (
                "/api/v1/crates/demo/owners".into(),
                (
                    200,
                    br#"{"users":[{"id":42,"login":"owner","kind":"user"}]}"#.to_vec(),
                ),
            ),
            (
                "/api/v1/crates/demo".into(),
                (200, versions.to_string().into_bytes()),
            ),
            ("/api/v1/crates/demo/0.1.1".into(), (404, vec![])),
        ]))
    };
    let collision = serde_json::json!({"crate":{"id":"demo","name":"demo","versions":[1]},"versions":[{"id":1,"crate":"demo","num":"0.1.1+other","yanked":true}]});
    assert!(inspect_crate_registry(&make(collision), "demo", "0.1.1").is_err());
    let incomplete = serde_json::json!({"crate":{"id":"demo","name":"demo","versions":[1,2]},"versions":[{"id":1,"crate":"demo","num":"0.1.0","yanked":false}]});
    assert!(inspect_crate_registry(&make(incomplete), "demo", "0.1.1").is_err());
}

#[test]
fn preflight_encodes_build_metadata_and_rejects_other_builds()
-> Result<(), Box<dyn std::error::Error>> {
    let owners = (
        200,
        br#"{"users":[{"id":42,"login":"owner","kind":"user"}]}"#.to_vec(),
    );
    let metadata = |number: &str| {
        (200, serde_json::json!({"crate":{"id":"demo","name":"demo","versions":[1]},"versions":[{"id":1,"crate":"demo","num":number,"yanked":false}]}).to_string().into_bytes())
    };
    let make = |number: &str| {
        Registry(BTreeMap::from([
            ("/api/v1/crates/demo/owners".into(), owners.clone()),
            ("/api/v1/crates/demo".into(), metadata(number)),
            ("/api/v1/crates/demo/0.1.1%2Bnew".into(), (404, vec![])),
        ]))
    };
    assert!(inspect_crate_registry(&make("0.1.1+old"), "demo", "0.1.1+new").is_err());
    assert_eq!(
        inspect_crate_registry(&make("0.1.0"), "demo", "0.1.1+new")?.1,
        velnor_actions_orchestrator::crate_release_registry::CrateRegistryPresence::Absent
    );
    Ok(())
}

#[test]
fn registry_accepts_crates_io_version_length_limit() -> Result<(), Box<dyn std::error::Error>> {
    let version = format!("1.2.3-{}", "a".repeat(144));
    assert_eq!(version.len(), 150);
    let reader = Registry(BTreeMap::from([
        ("/api/v1/crates/demo/owners".into(), (200, br#"{"users":[{"id":42,"login":"owner","kind":"user"}]}"#.to_vec())),
        ("/api/v1/crates/demo".into(), (200, br#"{"crate":{"id":"demo","name":"demo","versions":[1]},"versions":[{"id":1,"crate":"demo","num":"0.1.0","yanked":false}]}"#.to_vec())),
        (format!("/api/v1/crates/demo/{version}"), (404, vec![])),
    ]));
    assert_eq!(
        inspect_crate_registry(&reader, "demo", &version)?.1,
        velnor_actions_orchestrator::crate_release_registry::CrateRegistryPresence::Absent
    );
    assert!(inspect_crate_registry(&reader, "demo", &format!("{version}a")).is_err());
    Ok(())
}

#[test]
fn registry_preflight_rejects_wrong_version_detail_identity() {
    let make = |detail: serde_json::Value| {
        Registry(BTreeMap::from([
        ("/api/v1/crates/demo/owners".into(), (200, br#"{"users":[{"id":42,"login":"owner","kind":"user"}]}"#.to_vec())),
        ("/api/v1/crates/demo".into(), (200, br#"{"crate":{"id":"demo","name":"demo","versions":[1]},"versions":[{"id":1,"crate":"demo","num":"0.1.1","yanked":false}]}"#.to_vec())),
        ("/api/v1/crates/demo/0.1.1".into(), (200, detail.to_string().into_bytes())),
    ]))
    };
    for detail in [
        serde_json::json!({"version":{"id":2,"crate":"demo","num":"0.1.1","yanked":false}}),
        serde_json::json!({"version":{"id":1,"crate":"other","num":"0.1.1","yanked":false}}),
        serde_json::json!({"version":{"id":1,"crate":"demo","num":"0.1.2","yanked":false}}),
        serde_json::json!({"version":{"id":1,"crate":"demo","num":"0.1.1","yanked":true}}),
    ] {
        assert!(inspect_crate_registry(&make(detail), "demo", "0.1.1").is_err());
    }
}

#[test]
fn registry_preflight_treats_only_404_as_absent_and_requires_known_owners()
-> Result<(), Box<dyn std::error::Error>> {
    let owners = br#"{"users":[{"id":42,"login":"owner","kind":"user"}]}"#.to_vec();
    let owners_path = "/api/v1/crates/demo/owners".to_owned();
    let version_path = "/api/v1/crates/demo/0.1.1".to_owned();
    let reader = |status| {
        Registry(BTreeMap::from([
            (owners_path.clone(), (200, owners.clone())),
            (
                "/api/v1/crates/demo".into(),
                (
                    200,
                    if status == 200 {
                        br#"{"crate":{"id":"demo","name":"demo","versions":[1,2]},"versions":[{"id":1,"crate":"demo","num":"0.1.0","yanked":false},{"id":2,"crate":"demo","num":"0.1.1","yanked":false}]}"#.to_vec()
                    } else {
                        br#"{"crate":{"id":"demo","name":"demo","versions":[1]},"versions":[{"id":1,"crate":"demo","num":"0.1.0","yanked":false}]}"#.to_vec()
                    },
                ),
            ),
            (
                version_path.clone(),
                (
                    status,
                    br#"{"version":{"id":2,"crate":"demo","num":"0.1.1","yanked":false}}"#.to_vec(),
                ),
            ),
        ]))
    };
    let (
        velnor_actions_orchestrator::crate_release_registry::CrateRegistryOwnership::Existing {
            owners: expected_owners,
        },
        presence,
    ) = inspect_crate_registry(&reader(404), "demo", "0.1.1")?
    else {
        return Err("expected owners".into());
    };
    assert_eq!(
        presence,
        velnor_actions_orchestrator::crate_release_registry::CrateRegistryPresence::Absent
    );
    assert_eq!(
        inspect_crate_registry(&reader(200), "demo", "0.1.1")?.1,
        velnor_actions_orchestrator::crate_release_registry::CrateRegistryPresence::Present
    );
    for status in [403, 429, 500] {
        assert!(inspect_crate_registry(&reader(status), "demo", "0.1.1").is_err());
    }
    assert!(inspect_crate_registry(&reader(404), "../demo", "0.1.1").is_err());
    assert!(
        reconcile_crate_registry(&reader(404), "demo", "0.1.1", &expected_owners, b"anything")
            .is_err()
    );
    let changed = Registry(BTreeMap::from([(
        owners_path,
        (
            200,
            br#"{"users":[{"id":43,"login":"other","kind":"user"}]}"#.to_vec(),
        ),
    )]));
    assert!(
        reconcile_crate_registry(&changed, "demo", "0.1.1", &expected_owners, b"anything").is_err()
    );
    Ok(())
}

#[test]
fn reconciliation_requires_matching_archive_checksum_version_and_owners()
-> Result<(), Box<dyn std::error::Error>> {
    use flate2::{Compression, write::GzEncoder};
    use sha2::{Digest, Sha256};
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_size(3);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, "demo-0.1.1/src/lib.rs", &b"pub"[..])?;
    let manifest = b"[package]\nname = \"demo\"\nversion = \"0.1.1\"\n[features]\ndefault = []\n";
    let mut manifest_header = tar::Header::new_gnu();
    manifest_header.set_size(manifest.len() as u64);
    manifest_header.set_mode(0o644);
    manifest_header.set_cksum();
    builder.append_data(&mut manifest_header, "demo-0.1.1/Cargo.toml", &manifest[..])?;
    let archive = builder.into_inner()?.finish()?;
    let digest = Sha256::digest(&archive);
    let mut checksum = String::new();
    for b in digest {
        checksum.push(char::from(b"0123456789abcdef"[usize::from(b >> 4)]));
        checksum.push(char::from(b"0123456789abcdef"[usize::from(b & 15)]));
    }
    let owners = br#"{"users":[{"id":42,"login":"owner","kind":"user"}]}"#.to_vec();
    let metadata = |yanked| {
        serde_json::json!({"version":{"crate":"demo","num":"0.1.1","checksum":checksum,"yanked":yanked,"features":{"default":[]}}}).to_string().into_bytes()
    };
    let make = |yanked| {
        Registry(BTreeMap::from([
            ("/api/v1/crates/demo/owners".into(), (200, owners.clone())),
            ("/api/v1/crates/demo/0.1.1".into(), (200, metadata(yanked))),
            (
                "/crates/demo/demo-0.1.1.crate".into(),
                (200, archive.clone()),
            ),
        ]))
    };
    let (velnor_actions_orchestrator::crate_release_registry::CrateRegistryOwnership::Existing { owners: expected_owners }, _) = inspect_crate_registry(
        &Registry(BTreeMap::from([
            ("/api/v1/crates/demo/owners".into(), (200, owners.clone())),
            ("/api/v1/crates/demo".into(), (200, br#"{"crate":{"id":"demo","name":"demo","versions":[1]},"versions":[{"id":1,"crate":"demo","num":"0.1.0","yanked":false}]}"#.to_vec())),
            ("/api/v1/crates/demo/0.1.1".into(), (404, vec![])),
        ])),
        "demo",
        "0.1.1",
    )? else { return Err("expected owners".into()) };
    reconcile_crate_registry(&make(false), "demo", "0.1.1", &expected_owners, &archive)?;
    assert!(
        reconcile_crate_registry(&make(true), "demo", "0.1.1", &expected_owners, &archive).is_err()
    );
    Ok(())
}

#[test]
fn registry_propagation_waits_for_404_but_not_errors() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
    };
    use velnor_actions_orchestrator::crate_release_registry::wait_for_crate_version;
    struct Sequence(RefCell<VecDeque<u16>>);
    impl CrateRegistryRead for Sequence {
        fn get(&self, _path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
            Ok(CrateRegistryResponse {
                status: self.0.borrow_mut().pop_front().unwrap_or(500),
                body: Vec::new(),
            })
        }
    }
    let waits = Cell::new(0);
    let delayed = Sequence(RefCell::new(VecDeque::from([404, 404, 200])));
    wait_for_crate_version(&delayed, "demo", "0.1.1", 3, || waits.set(waits.get() + 1))?;
    assert_eq!(waits.get(), 2);
    let failed = Sequence(RefCell::new(VecDeque::from([403, 200])));
    assert!(
        wait_for_crate_version(&failed, "demo", "0.1.1", 3, || waits.set(waits.get() + 1)).is_err()
    );
    assert_eq!(waits.get(), 2);
    Ok(())
}

#[test]
fn new_crate_requires_consistent_not_found_responses() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_registry::CrateRegistryOwnership;
    let reply = |root, owners, version| {
        Registry(BTreeMap::from([
            ("/api/v1/crates/demo".into(), (root, vec![])),
            ("/api/v1/crates/demo/owners".into(), (owners, vec![])),
            ("/api/v1/crates/demo/0.1.1".into(), (version, vec![])),
        ]))
    };
    assert_eq!(
        inspect_crate_registry(&reply(404, 404, 404), "demo", "0.1.1")?,
        (
            CrateRegistryOwnership::NewCrate,
            velnor_actions_orchestrator::crate_release_registry::CrateRegistryPresence::Absent
        )
    );
    let existing = Registry(BTreeMap::from([
        ("/api/v1/crates/demo".into(), (200, br#"{"crate":{"id":"demo","name":"demo","versions":[1]},"versions":[{"id":1,"crate":"demo","num":"0.1.0","yanked":false}]}"#.to_vec())),
        ("/api/v1/crates/demo/owners".into(), (200, br#"{"users":[{"id":42,"login":"owner","kind":"user"}]}"#.to_vec())),
        ("/api/v1/crates/demo/0.1.1".into(), (404, vec![])),
    ]));
    assert!(
        matches!(inspect_crate_registry(&existing, "demo", "0.1.1"), Ok((CrateRegistryOwnership::Existing { owners }, _)) if owners.len() == 1 && owners[0].id == 42)
    );
    for statuses in [
        (404, 200, 404),
        (404, 403, 404),
        (404, 429, 404),
        (404, 500, 404),
        (404, 404, 200),
        (404, 404, 500),
        (500, 404, 404),
    ] {
        assert!(
            inspect_crate_registry(&reply(statuses.0, statuses.1, statuses.2), "demo", "0.1.1")
                .is_err(),
            "{statuses:?}"
        );
    }
    Ok(())
}
