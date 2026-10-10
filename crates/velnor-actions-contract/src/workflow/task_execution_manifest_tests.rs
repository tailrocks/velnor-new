//! Contract tests for generated declared-task manifests.

use std::collections::BTreeMap;

use super::*;
use crate::cachekey::{ToolchainInputs, toolchain_id};
use crate::workflow::crate_job::task_digest_for_execution;
use crate::{matrix_id_for_task_group, matrix_key_for_id};

fn entry() -> TaskExecutionManifestEntryV1 {
    let task_id = "stack/rust/demo/clippy/default".to_owned();
    let argv = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        "rust@1.99.0".to_owned(),
        "--".to_owned(),
        "cargo".to_owned(),
        "fmt".to_owned(),
        "--check".to_owned(),
    ];
    let toolchain_inputs = ToolchainInputs {
        tools: vec!["rust@1.99.0".to_owned()],
        components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let toolchain_id = toolchain_id(&toolchain_inputs).expect("toolchain id");
    let task_digest =
        task_digest_for_execution(&task_id, &argv, &toolchain_id).expect("plan task digest");
    let matrix_id = matrix_id_for_task_group("rust", &task_id).expect("matrix id");
    let matrix_key = matrix_key_for_id(&matrix_id).expect("matrix key");
    let mut env = BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.99.0".to_owned()),
    ]);
    env.insert("RUSTDOCFLAGS".to_owned(), "-D warnings".to_owned());
    let mut record = TaskExecutionManifestEntryV1 {
        task_id,
        execution_digest: String::new(),
        task_digest,
        toolchain_inputs,
        argv,
        env,
        matrix_id,
        matrix_key,
        report_helper_version: "0.1.7".to_owned(),
        matrix_max_parallel: Some(8),
    };
    record.refresh_execution_digest().expect("execution digest");
    record
}

fn manifest() -> TaskExecutionManifestV1 {
    let record = entry();
    TaskExecutionManifestV1 {
        schema: TASK_EXECUTION_MANIFEST_SCHEMA,
        generator_version: "0.1.7".to_owned(),
        tasks: BTreeMap::from([(record.task_id.clone(), record)]),
    }
}

#[test]
fn task_manifest_validates_separate_plan_and_execution_digests() {
    let value = manifest();
    value.validate().expect("valid manifest");
    let record = value.tasks.values().next().expect("entry");
    assert_ne!(record.execution_digest, record.task_digest);

    let mut changed = record.clone();
    changed
        .env
        .insert("RUSTDOCFLAGS".to_owned(), "-Dwarnings".to_owned());
    assert_ne!(
        changed.computed_execution_digest().expect("new digest"),
        record.execution_digest
    );
    changed.execution_digest = record.execution_digest.clone();
    assert!(
        changed.validate().is_err(),
        "stale execution digest rejected"
    );
}

#[test]
fn execution_digest_binds_all_record_metadata() {
    let original = entry();
    let digest = original.execution_digest.clone();
    let mut changed = original.clone();
    changed.task_id.push_str("/different-task");
    assert_ne!(changed.computed_execution_digest().unwrap(), digest);
    let mut changed = original.clone();
    changed.argv[9] = "--all".to_owned();
    assert_ne!(changed.computed_execution_digest().unwrap(), digest);
    let mut changed = original.clone();
    changed.task_digest = format!("b3-{}", "c".repeat(64));
    assert_ne!(changed.computed_execution_digest().unwrap(), digest);
    let mut changed = original.clone();
    changed.toolchain_inputs.tools[0] = "rust@1.98.0".to_owned();
    assert_ne!(changed.computed_execution_digest().unwrap(), digest);
    let mut changed = original.clone();
    changed.matrix_id.push_str("-changed");
    assert_ne!(changed.computed_execution_digest().unwrap(), digest);
    let mut changed = original.clone();
    changed.matrix_key.push_str("-changed");
    assert_ne!(changed.computed_execution_digest().unwrap(), digest);
    let mut changed = original.clone();
    changed.report_helper_version = "0.1.7".to_owned();
    assert_ne!(changed.computed_execution_digest().unwrap(), digest);
    let mut changed = original;
    changed.matrix_max_parallel = Some(9);
    assert_ne!(changed.computed_execution_digest().unwrap(), digest);
}

#[test]
fn manifest_key_schema_and_unknown_fields_fail_closed() {
    let mut value = manifest();
    value.validate().expect("valid manifest");
    let mut wrong_key = value.clone();
    let record = wrong_key.tasks.pop_first().expect("entry").1;
    wrong_key.tasks.insert("other-task".to_owned(), record);
    assert!(wrong_key.validate().is_err(), "map key mismatch rejected");

    value.schema += 1;
    assert!(value.validate().is_err(), "unknown version rejected");
    assert!(
        serde_json::from_str::<TaskExecutionManifestV1>(
            r#"{"schema":1,"generator_version":"0.1.7","tasks":{},"extra":true}"#
        )
        .is_err(),
        "unknown fields rejected"
    );
    assert!(
        crate::strict_json::parse_strict_json(
            r#"{"schema":1,"schema":1,"generator_version":"0.1.7","tasks":{}}"#
        )
        .is_err(),
        "duplicate fields rejected by the shared strict parser"
    );
}

#[test]
fn marked_json_and_nul_frame_have_exact_bounded_shapes() {
    let manifest = manifest();
    let marked = manifest.marked_json().expect("marked manifest");
    assert!(marked.starts_with(
        "# Generated by Velnor Actions 0.1.7; edit .velnor/config.toml and regenerate.\n"
    ));
    assert_eq!(marked.lines().count(), 2);

    let record = manifest.tasks.values().next().expect("record");
    let frame = record.nul_frame().expect("frame");
    assert!(frame.len() <= MAX_TASK_EXECUTION_FRAME_BYTES);
    assert_eq!(frame.last(), Some(&0));
    let fields = frame
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| std::str::from_utf8(field).expect("UTF-8 field"))
        .collect::<Vec<_>>();
    assert_eq!(fields[0], TASK_EXECUTION_FRAME_MAGIC);
    assert_eq!(fields[1], record.task_id);
    assert_eq!(fields[2], record.execution_digest);
    assert_eq!(fields[3], record.task_digest);
    assert_eq!(fields[7], "1");
    assert_eq!(fields[8], "8");
    assert_eq!(fields[9], record.argv.len().to_string());
    assert_eq!(fields[fields.len() - 1], "END");
}
