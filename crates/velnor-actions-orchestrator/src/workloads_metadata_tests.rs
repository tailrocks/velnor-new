//! Descriptor parsing preserves immutable authority and fails closed.

use std::collections::BTreeMap;
use velnor_actions_contract::config::{PostgresBinding, PostgresFixture, WorkloadConfig};

use super::{MAX_DESCRIPTOR_BYTES, POSTGRES_FIXTURE_KEY, fixture_identity, postgres_fixture};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn task(kind: &str, descriptor: Option<&str>) -> velnor_actions_contract::ProposedTask {
    let workload: WorkloadConfig = serde_json::from_value(serde_json::json!({
        "name": "database", "kind": "gradle_check"
    }))
    .expect("closed workload");
    let mut task = super::super::proposal(&workload, "check", vec!["check".to_owned()], None);
    task.configuration = kind.to_owned();
    if let Some(descriptor) = descriptor {
        task.identity
            .environment
            .insert(POSTGRES_FIXTURE_KEY.to_owned(), descriptor.to_owned());
    }
    task
}

fn fixture() -> PostgresFixture {
    PostgresFixture {
        user: "fixture".to_owned(),
        password: "local-fixture".to_owned(),
        databases: vec!["app".to_owned()],
        bindings: BTreeMap::from([
            ("APP_DB_USER".to_owned(), PostgresBinding::User),
            (
                "APP_DB_URL".to_owned(),
                PostgresBinding::JdbcUrl {
                    database: "app".to_owned(),
                },
            ),
        ]),
    }
}

#[test]
fn identical_typed_descriptors_allow_json_whitespace_and_both_gradle_kinds() -> TestResult {
    let fixture = fixture();
    let compact = serde_json::to_string(&fixture)?;
    let pretty = serde_json::to_string_pretty(&fixture)?;
    let check = task("gradle_check", Some(&compact));
    let database = task("gradle_database_check", Some(&pretty));
    assert_eq!(postgres_fixture(&[&check, &database])?, Some(fixture));
    Ok(())
}

#[test]
fn missing_and_explicitly_disabled_descriptors_disable_fixture() -> TestResult {
    assert_eq!(postgres_fixture(&[])?, None);
    assert_eq!(postgres_fixture(&[&task("gradle_check", None)])?, None);
    assert_eq!(
        postgres_fixture(&[&task("gradle_check", Some("null"))])?,
        None
    );
    Ok(())
}

#[test]
fn database_check_cannot_fall_back_to_ambient_database_without_fixture() {
    for descriptor in [None, Some("null")] {
        let mut task = task("gradle_database_check", descriptor);
        task.identity.environment.insert(
            "APP_DB_URL".to_owned(),
            "jdbc:postgresql://external.example:5432/production".to_owned(),
        );
        let error = postgres_fixture(&[&task]).expect_err("mandatory local fixture");
        assert!(error.to_string().contains("missing_required_fixture"));
        assert!(fixture_identity(&task).is_err());
    }
}

#[test]
fn every_group_member_requires_identical_descriptor_presence_and_values() -> TestResult {
    let mut changed = fixture();
    changed.password = "different".to_owned();
    let first = task("gradle_check", Some(&serde_json::to_string(&fixture())?));
    let absent = task("gradle_check", None);
    let disabled = task("gradle_check", Some("null"));
    let changed = task("gradle_check", Some(&serde_json::to_string(&changed)?));
    for pair in [
        [&first, &absent],
        [&absent, &first],
        [&first, &changed],
        [&absent, &disabled],
    ] {
        let error = postgres_fixture(&pair).expect_err("group descriptor mismatch");
        assert!(
            error
                .to_string()
                .contains("contradictory_group_descriptors")
        );
    }
    Ok(())
}

#[test]
fn malformed_or_unknown_json_never_disables_fixture() -> TestResult {
    let mut unknown = serde_json::to_value(fixture())?;
    unknown["command"] = serde_json::json!("curl attacker");
    for raw in ["{", "[]", "false", "{}", &unknown.to_string()] {
        let task = task("gradle_check", Some(raw));
        let error = postgres_fixture(&[&task]).expect_err("malformed descriptor");
        assert!(error.to_string().contains("malformed_descriptor"));
    }
    Ok(())
}

#[test]
fn present_fixture_requires_workload_stack_and_owning_kind() {
    for kind in ["node_ci", "bun_ci", "gradle_build", ""] {
        let task = task(kind, Some("null"));
        let error = postgres_fixture(&[&task]).expect_err("foreign descriptor");
        assert!(error.to_string().contains("wrong_workload_kind"));
    }
    let mut foreign = task("gradle_check", Some("null"));
    foreign.stack_id = "rust".to_owned();
    assert!(postgres_fixture(&[&foreign]).is_err());
}

#[test]
fn hostile_fixture_literals_and_binding_targets_are_rejected() -> TestResult {
    let mut cases = Vec::new();
    let mut external = fixture();
    external.password = "${{ secrets.PASSWORD }}".to_owned();
    cases.push(external);
    let mut identifier = fixture();
    identifier.user = "fixture;drop".to_owned();
    cases.push(identifier);
    let mut undeclared = fixture();
    undeclared.bindings.insert(
        "APP_DB_URL".to_owned(),
        PostgresBinding::JdbcUrl {
            database: "outside".to_owned(),
        },
    );
    cases.push(undeclared);
    for name in ["GITHUB_ENV", "LD_PRELOAD", "JAVA_TOOL_OPTIONS", "PATH"] {
        let mut binding = fixture();
        binding
            .bindings
            .insert(name.to_owned(), PostgresBinding::Host);
        cases.push(binding);
    }
    for fixture in cases {
        let task = task("gradle_check", Some(&serde_json::to_string(&fixture)?));
        assert!(postgres_fixture(&[&task]).is_err());
    }
    Ok(())
}

#[test]
fn raw_payload_never_supplies_fixture_and_metadata_key_stays_fixed() -> TestResult {
    assert_eq!(POSTGRES_FIXTURE_KEY, "VELNOR_GRADLE_POSTGRES_FIXTURE");
    let mut task = task("gradle_check", None);
    task.payload = vec![serde_json::to_string(&fixture())?.into()];
    assert_eq!(postgres_fixture(&[&task])?, None);
    Ok(())
}

#[test]
fn fixture_identity_is_canonical_validated_and_tracks_descriptor_changes() -> TestResult {
    let fixture = fixture();
    let compact = task("gradle_check", Some(&serde_json::to_string(&fixture)?));
    let pretty = task(
        "gradle_check",
        Some(&serde_json::to_string_pretty(&fixture)?),
    );
    let mut changed = fixture;
    changed.databases.push("extra".to_owned());
    let changed = task("gradle_check", Some(&serde_json::to_string(&changed)?));
    assert_eq!(fixture_identity(&compact)?, fixture_identity(&pretty)?);
    assert_ne!(fixture_identity(&compact)?, fixture_identity(&changed)?);
    assert_eq!(fixture_identity(&task("gradle_check", None))?, None);
    assert!(fixture_identity(&task("gradle_check", Some("{}"))).is_err());
    Ok(())
}

#[test]
fn different_workload_identities_cannot_share_fixture_group() -> TestResult {
    let first = task("gradle_check", Some(&serde_json::to_string(&fixture())?));
    for field in ["unit_id", "unit_path", "project_root"] {
        let mut other = first.clone();
        match field {
            "unit_id" => other.identity.unit_id = "another".to_owned(),
            "unit_path" => other.identity.unit_path = "another".to_owned(),
            _ => other.identity.project_root = "another".to_owned(),
        }
        let error = postgres_fixture(&[&first, &other]).expect_err("mixed identity");
        assert!(error.to_string().contains("mixed_group_identity"));
    }
    Ok(())
}

#[test]
fn oversized_fixture_metadata_is_rejected_before_json_decoding() {
    let raw = " ".repeat(MAX_DESCRIPTOR_BYTES + 1);
    let task = task("gradle_check", Some(&raw));
    let error = postgres_fixture(&[&task]).expect_err("descriptor bound");
    assert!(error.to_string().contains("descriptor_size"));
}
