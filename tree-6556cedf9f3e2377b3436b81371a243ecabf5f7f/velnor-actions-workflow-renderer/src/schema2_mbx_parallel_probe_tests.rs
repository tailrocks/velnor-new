use std::collections::BTreeMap;
use std::process::Command;

use crate::schema2::MbxQualificationPins;
use crate::setup::MiseSetup;
use crate::yaml::Yaml;

use super::{NEW_KEY_SCOPE, PARALLEL_GATE, Role, SHARED_SCOPE, jobs};

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
    assert_observer_scope_and_order(&jobs);
    assert_observer_script_semantics(&jobs);
}

fn assert_observer_scope_and_order(jobs: &[(String, Yaml)]) {
    let observer = job(jobs, Role::ObserverShared.id());
    let prepare_dirs = step(observer, "Prepare private MBX receipt directories");
    assert!(string(field(prepare_dirs, "run")).contains("mkdir -m 700"));
    let api_step = step(observer, "Validate MBX parallel REST timestamps");
    assert_eq!(string(field(api_step, "shell")), "bash");
    let api_env = map_fields(field(api_step, "env"));
    assert_eq!(string(field(api_env, "GH_TOKEN")), "${{ github.token }}");
    for (key, value) in [
        (
            "MBX_PARALLEL_OBSERVER_PRIMARY",
            "${{ steps.mbx-bundle-key.outputs.primary }}",
        ),
        (
            "MBX_PARALLEL_OBSERVER_RESTORE_PRIMARY",
            "${{ steps.mbx-bundle.outputs.cache-primary-key }}",
        ),
        (
            "MBX_PARALLEL_OBSERVER_CACHE_HIT",
            "${{ steps.mbx-bundle.outputs.cache-hit }}",
        ),
        (
            "MBX_PARALLEL_OBSERVER_MATCHED_KEY",
            "${{ steps.mbx-bundle.outputs.cache-matched-key }}",
        ),
        (
            "MBX_PARALLEL_OBSERVER_RESTORE_CONCLUSION",
            "${{ steps.mbx-bundle.conclusion }}",
        ),
    ] {
        assert_eq!(string(field(api_env, key)), value);
    }
    assert_eq!(
        string(field(map_fields(field(observer, "permissions")), "actions")),
        "read"
    );
    let Yaml::Seq(observer_steps) = field(observer, "steps") else {
        panic!("steps must be a sequence");
    };
    assert_receipt_directories_precede_downloads(observer_steps);
    let api_position = observer_steps.iter().position(|candidate| {
        string(field(map_fields(candidate), "name")) == "Validate MBX parallel REST timestamps"
    });
    let reuse_position = observer_steps
        .iter()
        .position(|candidate| string(field(map_fields(candidate), "name")) == "Require reused compilation");
    assert!(api_position.is_some_and(|api| reuse_position.is_some_and(|reuse| api == reuse + 1)));
    for role in [
        Role::Seed,
        Role::ReaderA,
        Role::ReaderB,
        Role::NewKeyWriter,
        Role::ObserverNew,
    ] {
        assert!(
            optional_step(
                job(jobs, role.id()),
                "Validate MBX parallel REST timestamps"
            )
            .is_none()
        );
    }
    for (id, fields) in jobs {
        let Yaml::Seq(steps) = field(map_fields(fields), "steps") else {
            panic!("steps must be a sequence");
        };
        for candidate in steps {
            let candidate = map_fields(candidate);
            if let Some(environment) = optional_field(candidate, "env") {
                let scoped_token = optional_field(map_fields(environment), "GH_TOKEN")
                    .is_some_and(|value| string(value) == "${{ github.token }}");
                assert_eq!(
                    scoped_token,
                    id == Role::ObserverShared.id()
                        && string(field(candidate, "name"))
                            == "Validate MBX parallel REST timestamps"
                );
            }
        }
    }
}

fn assert_receipt_directories_precede_downloads(observer_steps: &[Yaml]) {
    let prepare_position = observer_steps.iter().position(|candidate| {
        string(field(map_fields(candidate), "name")) == "Prepare private MBX receipt directories"
    });
    for receipt_step in [
        "Download MBX seed receipt",
        "Download MBX reader-a receipt",
        "Download MBX reader-b receipt",
        "Download MBX new-key-writer receipt",
    ] {
        let download_position = observer_steps.iter().position(|candidate| {
            string(field(map_fields(candidate), "name")) == receipt_step
        });
        assert!(prepare_position.is_some_and(|prepare| {
            download_position.is_some_and(|download| prepare < download)
        }));
    }
}

fn assert_observer_script_semantics(jobs: &[(String, Yaml)]) {
    let observer = job(jobs, Role::ObserverShared.id());
    let api_step = step(observer, "Validate MBX parallel REST timestamps");
    let script = string(field(api_step, "run"));
    let bash_check = Command::new("bash")
        .args(["-n", "-c", script])
        .output()
        .expect("Bash is available for syntax validation");
    assert!(
        bash_check.status.success(),
        "Bash rejected the receipt script: {}",
        String::from_utf8_lossy(&bash_check.stderr)
    );
    assert!(script.contains('\n'));
    assert!(script.contains("stock_restore_classify_receipt"));
    assert!(script.contains("mbx-stock-restore-evidence"));
    assert!(script.contains("CLEAN_MISS"));
    assert!(script.contains("/attempts/$GITHUB_RUN_ATTEMPT/jobs"));
    assert!(script.contains("Restore MBX single bundle"));
    assert!(script.contains("Compile MBX cache probe"));
    assert!(script.contains("NOT_RUN"));
    assert!(script.contains("seed_save_not_complete_before_parallel_starts"));
    assert!(script.contains("qualification.yml@refs/heads/main"));
    assert!(script.contains("$GITHUB_WORKFLOW_REF"));
    assert!(script.contains("($seed_save.completed_at | epoch_ms)"));
    assert!(
        script
            .contains("[$restore_a.started_at, $restore_b.started_at, $restore_writer.started_at]")
    );
    assert!(
        script
            .contains("[$build_a.completed_at, $build_b.completed_at, $build_writer.completed_at]")
    );
    assert!(script.contains("$latest_start < $earliest_end"));
    assert!(script.contains("overlap_duration_ms"));
    assert!(!script.contains("workflow-runs?"));
    assert!(!script.contains("actions/runs?"));
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
