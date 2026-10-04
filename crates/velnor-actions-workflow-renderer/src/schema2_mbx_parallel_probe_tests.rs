use crate::schema2::MbxQualificationPins;
use crate::setup::MiseSetup;
use crate::yaml::Yaml;
use std::collections::BTreeMap;

use super::{NEW_KEY_SCOPE, PARALLEL_GATE, Role, SHARED_SCOPE, jobs};

#[path = "schema2_mbx_parallel_probe_tests_observer_assertions.rs"]
mod observer_assertions;

fn pins() -> MbxQualificationPins {
    MbxQualificationPins {
        mise_setup: MiseSetup {
            uses: format!("jdx/mise-action@{}", "a".repeat(40)),
            version: "2026.9.18".to_owned(),
            sha256: "b".repeat(64),
        },
        mbx_action_uses: format!("jdx/mr-boxington-action@{}", "c".repeat(40)),
        mbx_version: "1.22.0".to_owned(),
        rust_version: "1.98.1".to_owned(),
    }
}

fn rendered_jobs() -> Vec<(String, Yaml)> {
    jobs(&pins(), &Yaml::str("ubuntu-26.04")).expect("parallel jobs render")
}

fn map_fields(value: &Yaml) -> &[(String, Yaml)] {
    match value {
        Yaml::Map(fields) => fields,
        other => panic!("expected mapping, got {other:?}"),
    }
}

fn field<'a>(fields: &'a [(String, Yaml)], name: &str) -> &'a Yaml {
    fields
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value))
        .unwrap_or_else(|| panic!("missing field {name}"))
}

fn optional_field<'a>(fields: &'a [(String, Yaml)], name: &str) -> Option<&'a Yaml> {
    fields
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value))
}

fn job<'a>(jobs: &'a [(String, Yaml)], id: &str) -> &'a [(String, Yaml)] {
    map_fields(
        jobs.iter()
            .find_map(|(key, value)| (key == id).then_some(value))
            .unwrap_or_else(|| panic!("missing job {id}")),
    )
}

fn step<'a>(job: &'a [(String, Yaml)], name: &str) -> &'a [(String, Yaml)] {
    optional_step(job, name).unwrap_or_else(|| panic!("missing step {name}"))
}

fn optional_step<'a>(job: &'a [(String, Yaml)], name: &str) -> Option<&'a [(String, Yaml)]> {
    let Yaml::Seq(steps) = field(job, "steps") else {
        panic!("steps must be a sequence");
    };
    steps.iter().find_map(|step| {
        let fields = map_fields(step);
        (string(field(fields, "name")) == name).then_some(fields)
    })
}

fn string(value: &Yaml) -> &str {
    match value {
        Yaml::Str(value) => value,
        other => panic!("expected string, got {other:?}"),
    }
}

fn job_ids() -> [Role; 6] {
    [
        Role::Seed,
        Role::ReaderA,
        Role::ReaderB,
        Role::NewKeyWriter,
        Role::ObserverShared,
        Role::ObserverNew,
    ]
}

#[test]
fn parallel_jobs_use_the_seed_then_independent_readers_and_writer() {
    let jobs = rendered_jobs();
    assert_eq!(jobs.len(), 6);
    for role in job_ids() {
        let fields = job(&jobs, role.id());
        assert_eq!(string(field(fields, "if")), PARALLEL_GATE);
        assert_eq!(string(field(fields, "runs-on")), "ubuntu-26.04");
        assert!(matches!(field(fields, "timeout-minutes"), Yaml::Int(45)));
    }
    assert!(optional_field(job(&jobs, Role::Seed.id()), "needs").is_none());
}

#[test]
fn parallel_job_dag_has_no_false_serialization() {
    let jobs = rendered_jobs();
    for role in [Role::ReaderA, Role::ReaderB, Role::NewKeyWriter] {
        let needs = field(job(&jobs, role.id()), "needs");
        let Yaml::Seq(needs) = needs else {
            panic!("parallel job needs must be a sequence");
        };
        assert_eq!(needs.len(), 1);
        assert_eq!(string(&needs[0]), Role::Seed.id());
    }
    for role in [Role::ObserverShared, Role::ObserverNew] {
        let needs = field(job(&jobs, role.id()), "needs");
        let Yaml::Seq(needs) = needs else {
            panic!("observer needs must be a sequence");
        };
        assert_eq!(
            needs.iter().map(string).collect::<Vec<_>>(),
            [
                Role::Seed.id(),
                Role::ReaderA.id(),
                Role::ReaderB.id(),
                Role::NewKeyWriter.id()
            ]
        );
    }
}

#[test]
fn parallel_roles_use_reserved_run_scopes_and_stock_local_mbx() {
    let jobs = rendered_jobs();
    let scopes = BTreeMap::from([
        (Role::Seed, SHARED_SCOPE),
        (Role::ReaderA, SHARED_SCOPE),
        (Role::ReaderB, SHARED_SCOPE),
        (Role::NewKeyWriter, NEW_KEY_SCOPE),
        (Role::ObserverShared, SHARED_SCOPE),
        (Role::ObserverNew, NEW_KEY_SCOPE),
    ]);
    for role in job_ids() {
        let job = job(&jobs, role.id());
        let setup = step(job, "Setup MBX");
        assert_eq!(
            string(field(map_fields(field(setup, "with")), "backend")),
            "local"
        );
        let setup_inputs = map_fields(field(setup, "with"));
        assert!(optional_field(setup_inputs, "velnor-cache-scope").is_none());
        assert!(optional_field(setup_inputs, "velnor-cache-writer").is_none());
        let key_env = map_fields(field(step(job, "Prepare MBX bundle key"), "env"));
        assert_eq!(string(field(key_env, "MBX_CACHE_SCOPE")), scopes[&role]);
        let job_env = map_fields(field(job, "env"));
        assert_eq!(string(field(job_env, "MBX_CACHE_SCOPE")), scopes[&role]);
        assert_eq!(
            string(field(job_env, "MBX_QUALIFICATION_ACTION_REF")),
            pins().mbx_action_uses
        );
        let restore = step(job, "Restore MBX single bundle");
        assert!(
            map_fields(field(restore, "with"))
                .iter()
                .all(|(key, _)| key != "restore-keys")
        );
    }
}

#[test]
fn reader_and_writer_guards_are_explicit_and_permissions_are_narrow() {
    let jobs = rendered_jobs();
    let seed = job(&jobs, Role::Seed.id());
    let cold_guard = string(field(step(seed, "Import MBX single bundle"), "run"));
    assert!(cold_guard.contains("qualification writer restore was not cold"));
    for role in [Role::ReaderA, Role::ReaderB] {
        let fields = job(&jobs, role.id());
        let guard = string(field(step(fields, "Import MBX single bundle"), "run"));
        assert!(guard.contains("qualification reader restore was not an exact cache hit"));
        let import_probe = step(fields, "Require imported MBX objects");
        assert!(string(field(import_probe, "run")).contains(".objects > 0"));
        let permissions = map_fields(field(fields, "permissions"));
        assert_eq!(string(field(permissions, "actions")), "read");
    }
    for role in [Role::Seed, Role::NewKeyWriter] {
        assert_eq!(
            string(field(
                map_fields(field(job(&jobs, role.id()), "permissions")),
                "actions"
            )),
            "write"
        );
    }
    for role in [Role::ObserverShared, Role::ObserverNew] {
        assert_eq!(
            string(field(
                map_fields(field(job(&jobs, role.id()), "permissions")),
                "actions"
            )),
            "read"
        );
    }
}

#[test]
fn qualification_writers_save_only_after_a_cold_exact_restore() {
    let jobs = rendered_jobs();
    for role in [Role::Seed, Role::NewKeyWriter] {
        let fields = job(&jobs, role.id());
        let export = step(fields, "Export MBX single bundle");
        let save = step(fields, "Save MBX single bundle");
        let export_gate = string(field(export, "if"));
        let save_gate = string(field(save, "if"));
        assert!(export_gate.contains(PARALLEL_GATE));
        assert!(export_gate.contains("steps.mbx-bundle.outputs.cache-hit != 'true'"));
        assert!(save_gate.contains(PARALLEL_GATE));
        assert!(save_gate.contains("steps.mbx-bundle.outputs.cache-hit != 'true'"));
        assert!(save_gate.contains("steps.mbx-export.outputs.ready == 'true'"));
    }
    for role in [
        Role::ReaderA,
        Role::ReaderB,
        Role::ObserverShared,
        Role::ObserverNew,
    ] {
        let fields = job(&jobs, role.id());
        assert!(optional_step(fields, "Export MBX single bundle").is_none());
        assert!(optional_step(fields, "Save MBX single bundle").is_none());
    }
}

#[test]
fn api_observer_binds_current_attempt_and_records_relevant_overlap() {
    let jobs = rendered_jobs();
    observer_assertions::assert_observer_scope_and_order(&jobs);
    observer_assertions::assert_observer_script_semantics(&jobs);
}

#[test]
fn every_probe_uploads_pinned_job_evidence() {
    let jobs = rendered_jobs();
    for role in job_ids() {
        let fields = job(&jobs, role.id());
        let Yaml::Seq(steps) = field(fields, "steps") else {
            panic!("steps must be a sequence");
        };
        assert!(steps.iter().any(|step| {
            let fields = map_fields(step);
            string(field(fields, "name")) == "Upload MBX cache evidence"
                && string(field(fields, "uses"))
                    == "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"
        }));
    }
}

#[path = "schema2_mbx_parallel_probe_api_tests.rs"]
mod api_tests;
