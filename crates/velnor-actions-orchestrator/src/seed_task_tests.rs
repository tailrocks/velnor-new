//! `resolve-seed-task-v1`: lane-bound frames with machine-output flags.

use std::collections::BTreeMap;

use tempfile::TempDir;
use velnor_actions_contract::cachekey::{ToolchainInputs, toolchain_id};
use velnor_actions_contract::workflow::crate_job::task_digest_for_execution;
use velnor_actions_contract::workflow::{
    TASK_EXECUTION_MANIFEST_PATH, TASK_EXECUTION_MANIFEST_SCHEMA, TaskExecutionManifestEntryV1,
    TaskExecutionManifestV1,
};

use super::resolve_seed_task_to;

const VERSION: &str = "0.1.7";

fn base_env() -> BTreeMap<String, String> {
    [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("MISE_RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
        ("MISE_CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
        ("RUSTUP_TOOLCHAIN", "1.99.0"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}

fn record(task_id: &str, tail: &[&str], env: &[(&str, &str)]) -> TaskExecutionManifestEntryV1 {
    let mut argv: Vec<String> = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.99.0",
        "--",
        "cargo",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    argv.extend(tail.iter().map(|arg| (*arg).to_owned()));
    let toolchain_inputs = ToolchainInputs {
        tools: vec!["rust@1.99.0".to_owned()],
        components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let toolchain = toolchain_id(&toolchain_inputs).expect("toolchain");
    let task_digest = task_digest_for_execution(task_id, &argv, &toolchain).expect("task digest");
    let matrix_id =
        velnor_actions_contract::matrix_id_for_task_group("rust", task_id).expect("matrix id");
    let matrix_key = velnor_actions_contract::matrix_key_for_id(&matrix_id).expect("matrix key");
    let mut record = TaskExecutionManifestEntryV1 {
        task_id: task_id.to_owned(),
        execution_digest: String::new(),
        task_digest,
        toolchain_inputs,
        argv,
        env: base_env()
            .into_iter()
            .chain(
                env.iter()
                    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
            )
            .collect(),
        matrix_id,
        matrix_key,
        report_helper_version: VERSION.to_owned(),
        matrix_max_parallel: None,
    };
    record.refresh_execution_digest().expect("digest");
    record
}

fn fixture() -> (
    TempDir,
    TempDir,
    BTreeMap<String, TaskExecutionManifestEntryV1>,
) {
    let records = [
        record(
            "stack/rust/demo/build/default",
            &[
                "nextest",
                "list",
                "--profile",
                "ci",
                "--manifest-path",
                "demo/Cargo.toml",
            ],
            &[],
        ),
        record(
            "stack/rust/demo/clippy/default",
            &[
                "clippy",
                "--locked",
                "--manifest-path",
                "demo/Cargo.toml",
                "--",
                "-D",
                "warnings",
            ],
            &[("CARGO_TARGET_DIR", "target/velnor-clippy")],
        ),
        record(
            "stack/rust/demo/doc/default",
            &["doc", "--no-deps", "--manifest-path", "demo/Cargo.toml"],
            &[],
        ),
        record(
            "stack/rust/demo/fmt/default",
            &["fmt", "--check", "--manifest-path", "demo/Cargo.toml"],
            &[],
        ),
    ];
    let tasks: BTreeMap<String, TaskExecutionManifestEntryV1> = records
        .into_iter()
        .map(|record| (record.task_id.clone(), record))
        .collect();
    let manifest = TaskExecutionManifestV1 {
        schema: TASK_EXECUTION_MANIFEST_SCHEMA,
        generator_version: VERSION.to_owned(),
        tasks: tasks.clone(),
    };
    let repo = TempDir::new().expect("repo");
    let path = repo.path().join(TASK_EXECUTION_MANIFEST_PATH);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    std::fs::write(&path, manifest.marked_json().expect("json")).expect("manifest");
    (repo, TempDir::new().expect("runner temp"), tasks)
}

fn fields(frame: &[u8]) -> Vec<String> {
    assert_eq!(frame.last(), Some(&0));
    frame[..frame.len() - 1]
        .split(|byte| *byte == 0)
        .map(|field| String::from_utf8(field.to_vec()).expect("utf8"))
        .collect()
}

fn resolve(lane: &str, task: &str) -> Result<Vec<String>, String> {
    let (repo, temp, tasks) = fixture();
    let digest = &tasks[task].execution_digest;
    resolve_seed_task_to(repo.path(), temp.path(), digest, VERSION, lane)
        .map(|frame| fields(&frame))
        .map_err(|err| err.to_string())
}

#[test]
fn build_lane_inserts_the_cargo_message_format_after_nextest_list() {
    let frame = resolve("build", "stack/rust/demo/build/default").expect("frame");
    assert!(frame.contains(&"RUSTUP_TOOLCHAIN=1.99.0".to_owned()));
    assert!(
        frame
            .iter()
            .any(|field| field.starts_with("MISE_CARGO_HOME=/"))
    );
    assert!(frame.iter().all(|field| !field.contains("${{")));
    let at = frame
        .iter()
        .position(|field| field == "list")
        .expect("list");
    assert_eq!(frame[at - 1], "nextest");
    assert_eq!(
        frame[at + 1..at + 3],
        ["--cargo-message-format", "json-render-diagnostics"]
    );
    assert_eq!(frame[at + 3], "--profile");
}

#[test]
fn clippy_and_doc_lanes_keep_going_with_json_messages_before_any_separator() {
    let (repo, temp, tasks) = fixture();
    let digest = &tasks["stack/rust/demo/clippy/default"].execution_digest;
    let frame = fields(
        &resolve_seed_task_to(repo.path(), temp.path(), digest, VERSION, "clippy").expect("clippy"),
    );
    assert_eq!(frame[0], "CARGO_TARGET_DIR=target/velnor-clippy");
    let home = temp.path().join("velnor/cargo");
    assert!(frame.contains(&format!("MISE_CARGO_HOME={}", home.display())));
    assert_eq!(
        frame[1], "MISE_AUTO_INSTALL=false",
        "env entries come sorted by key"
    );
    let at = frame
        .iter()
        .position(|field| field == "clippy")
        .expect("clippy");
    assert_eq!(
        frame[at + 1..at + 4],
        [
            "--message-format",
            "json-render-diagnostics",
            "--keep-going"
        ]
    );
    assert_eq!(frame.last().map(String::as_str), Some("warnings"));
    let doc = resolve("doc", "stack/rust/demo/doc/default").expect("doc");
    let at = doc.iter().position(|field| field == "doc").expect("doc");
    assert_eq!(doc[at + 3], "--keep-going");
}

#[test]
fn records_resolve_only_in_their_own_lane_and_never_for_other_kinds() {
    for (lane, task) in [
        ("clippy", "stack/rust/demo/build/default"),
        ("build", "stack/rust/demo/doc/default"),
        ("build", "stack/rust/demo/fmt/default"),
        ("fmt", "stack/rust/demo/fmt/default"),
    ] {
        let err = resolve(lane, task).expect_err("rejected");
        assert!(
            err.contains("seed_record_lane_mismatch") || err.contains("unknown_seed_lane"),
            "{lane} {task}: {err}"
        );
    }
}

#[test]
fn a_wrong_version_or_unknown_digest_fails_closed() {
    let (repo, temp, tasks) = fixture();
    let digest = &tasks["stack/rust/demo/doc/default"].execution_digest;
    assert!(resolve_seed_task_to(repo.path(), temp.path(), digest, "0.1.8", "doc").is_err());
    let unknown = format!("b3-{}", "0".repeat(64));
    assert!(resolve_seed_task_to(repo.path(), temp.path(), &unknown, VERSION, "doc").is_err());
}
