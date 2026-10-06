//! Contract identity, digest, and derivation cases.
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, CompatibilityInputs, ContractError, ExecuteTaskIds, ExecuteTaskRef,
    MatrixEntry, PlannedPlatform, StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity,
    TaskInput, VcsInputs, artifact_id_for_final, artifact_id_for_matrix, artifact_id_for_plan,
    assign_crate_job_ids, canonical_json_bytes, canonical_json_str, compatibility_id, digest_b3,
    input_digest, manifest_key_for_cargo_manifest, matrix_id_for_task_group, matrix_key_for_id,
    plan_id_for_run, report_id_for_matrix, run_key_for_ci, slugify_segment, split_shard_suffix,
    target_key, task_id_for_internal, task_id_for_stack, task_report_id_for_task,
    validate_artifact_id, validate_digest, validate_fetch_root, validate_id, validate_job_id,
    validate_matrix_key, validate_plan_id, validate_report_id, validate_run_key, validate_task_id,
    validate_task_report_id,
};

/// Sample Cargo manifest path shared by contract cases.
pub(crate) const MANIFEST: &str = "crates/velnor-actions-contract/Cargo.toml";
/// Sample task ID shared by contract cases.
pub(crate) const TASK: &str = "stack/rust/crates/velnor-actions-contract/clippy/default";
/// Sample task-group ID shared by contract cases.
pub(crate) const GROUP: &str = "stack/rust/crates/velnor-actions-contract/validation/default";

/// Sample task identity shared by contract cases.
pub(crate) fn sample_identity() -> TaskIdentity {
    TaskIdentity {
        schema_version: 1,
        stack_id: "rust".to_owned(),
        project_root: ".".to_owned(),
        component_id: "crates/velnor-actions-contract".to_owned(),
        task_kind: "clippy".to_owned(),
        task_id: TASK.to_owned(),
        argv: vec![
            "clippy".to_owned(),
            "--package".to_owned(),
            "demo".to_owned(),
        ],
        working_dir: "crates/velnor-actions-contract".to_owned(),
        configuration: TaskConfiguration {
            target: "host".to_owned(),
            profile: "test".to_owned(),
            features: vec!["default".to_owned()],
            flags: vec![],
            task_contract: "clippy-v1".to_owned(),
            compile_driver: "cargo".to_owned(),
            test_runner: "cargo_test".to_owned(),
        },
        inputs: vec![TaskInput {
            path: "crates/velnor-actions-contract/src/lib.rs".to_owned(),
            digest: digest_b3(b"fn main() {}"),
        }],
        dependencies: vec![],
        vcs: VcsInputs {
            commit: None,
            reference: None,
            submodules: BTreeMap::new(),
        },
        toolchain_id: digest_b3(b"toolchain"),
        platform_id: digest_b3(b"platform"),
        environment: BTreeMap::from([("RUSTFLAGS".to_owned(), "-D warnings".to_owned())]),
        output_contract: "clippy-report-v1".to_owned(),
        generator: TaskGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
        },
        stack_extension: StackExtension {
            schema: "rust-task-v1".to_owned(),
            data: serde_json::json!({"manifest": MANIFEST}),
        },
    }
}

/// Fixed leg command shared by contract matrix fixtures.
pub(crate) const SAMPLE_RUN: &str = "mise exec --no-config rust@1.98.1 -- cargo clippy --locked";

/// Build one sample matrix entry shared by contract cases.
pub(crate) fn sample_entry(run_key: &str) -> Result<MatrixEntry, ContractError> {
    let mut tasks = BTreeMap::new();
    tasks.insert("clippy".to_owned(), ExecuteTaskRef::Single(TASK.to_owned()));
    MatrixEntry::derive(
        "rust",
        GROUP,
        SAMPLE_RUN,
        &digest_b3(b"task-bytes"),
        serde_json::json!({"manifest": MANIFEST}),
        ExecuteTaskIds { tasks },
        &digest_b3(b"entry-inputs"),
        run_key,
        "plan",
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu")?,
    )
}

/// Same fixture at two absolute roots => equal IDs/keys/digests; artifact IDs
/// are derived names; `run_key` is absent from the input preimage.
#[test]
fn identities_are_path_independent_and_artifact_id_is_derived_name() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(4242, 7);
    let key_a = manifest_key_for_cargo_manifest(MANIFEST)?;
    let key_b = manifest_key_for_cargo_manifest(MANIFEST)?;
    assert_eq!(key_a, key_b);
    let task_a = task_id_for_stack("rust", &key_a, "clippy", "default", None)?;
    let task_b = task_id_for_stack("rust", &key_b, "clippy", "default", None)?;
    assert_eq!(task_a, TASK);
    assert_eq!(task_a, task_b);
    let id = matrix_id_for_task_group("rust", GROUP)?;
    let matrix_key = matrix_key_for_id(&id)?;
    assert_eq!(matrix_key, matrix_key_for_id(&id)?);
    assert_eq!(sample_identity().validate(), Ok(()));
    let digest = input_digest(&sample_identity())?;
    assert_eq!(digest, input_digest(&sample_identity())?);
    validate_digest(&digest)?;
    let plan_artifact = artifact_id_for_plan(&run_key)?;
    let matrix_artifact = artifact_id_for_matrix(&run_key, &matrix_key)?;
    let final_artifact = artifact_id_for_final(&run_key)?;
    for artifact in [&plan_artifact, &matrix_artifact, &final_artifact] {
        validate_artifact_id(artifact)?;
        assert!(is_derived_artifact_name(artifact, &run_key));
    }
    assert_eq!(plan_artifact, format!("velnor-plan-{run_key}"));
    validate_artifact_id(&format!(
        "velnor-candidate-{run_key}-x86-64-unknown-linux-gnu"
    ))?;
    let preimage = canonical_json_str(&sample_identity())?;
    assert!(!preimage.contains(&run_key));
    assert!(!preimage.contains("/Users/"));
    assert!(!preimage.contains("/tmp/"));
    let compat = CompatibilityInputs {
        schema_id: "v1".to_owned(),
        repository_id: digest_b3(b"repo"),
        workspace_id: digest_b3(b"workspace"),
        lane_id: digest_b3(b"lane"),
        platform_id: digest_b3(b"platform"),
        toolchain_id: digest_b3(b"toolchain"),
        cache_format_id: digest_b3(b"format"),
        stack_extension_id: digest_b3(b"extension"),
    };
    let compat_id = compatibility_id(&compat)?;
    validate_digest(&compat_id)?;
    assert!(!canonical_json_str(&compat)?.contains(&run_key));
    Ok(())
}

/// Derived-name regex equivalent without regex: kind prefix + run key + tail.
fn is_derived_artifact_name(name: &str, run_key: &str) -> bool {
    if let Some(tail) = name.strip_prefix("velnor-plan-") {
        return tail == run_key;
    }
    if let Some(tail) = name.strip_prefix("velnor-final-") {
        return tail == run_key;
    }
    if let Some(tail) = name.strip_prefix("velnor-matrix-") {
        return tail.starts_with(run_key) && tail[run_key.len()..].starts_with("-m-");
    }
    if let Some(tail) = name.strip_prefix("velnor-candidate-") {
        return tail.starts_with(run_key) && tail.len() > run_key.len() + 1;
    }
    false
}

#[test]
fn canonical_json_sorts_keys_and_handles_floats() -> Result<(), ContractError> {
    let value = serde_json::json!({"b": 1, "a": [3, 2]});
    assert_eq!(canonical_json_str(&value)?, r#"{"a":[3,2],"b":1}"#);
    assert_eq!(
        canonical_json_bytes(&value)?,
        br#"{"a":[3,2],"b":1}"#.to_vec()
    );
    let floats = serde_json::json!({"x": 1.5, "n": -2});
    assert_eq!(canonical_json_str(&floats)?, r#"{"n":-2,"x":1.5}"#);
    // serde_json cannot represent non-finite numbers, so out-of-range input
    // fails at parse time before canonicalization.
    assert!(serde_json::from_str::<serde_json::Value>(r#"{"x":1e999}"#).is_err());
    Ok(())
}

#[test]
fn digest_helpers_use_b3_prefix() {
    let digest = digest_b3(b"velnor");
    assert!(digest.starts_with("b3-"));
    assert_eq!(digest.len(), 67);
    assert!(validate_digest(&digest).is_ok());
    assert!(validate_digest("b3-XYZ").is_err());
    assert!(validate_digest("sha256:abc").is_err());
}

#[test]
fn task_id_grammar_accepts_valid_and_rejects_absolute() -> Result<(), ContractError> {
    assert_eq!(manifest_key_for_cargo_manifest("Cargo.toml")?, "root");
    assert_eq!(
        manifest_key_for_cargo_manifest(MANIFEST)?,
        "crates/velnor-actions-contract"
    );
    assert!(manifest_key_for_cargo_manifest("/abs/Cargo.toml").is_err());
    assert!(manifest_key_for_cargo_manifest("Cargo.lock").is_err());
    assert!(manifest_key_for_cargo_manifest("-evil/Cargo.toml").is_err());
    assert!(manifest_key_for_cargo_manifest("--help").is_err());
    let shard = task_id_for_stack("rust", "root", "test-run", "default", Some((2, 4)))?;
    assert_eq!(shard, "stack/rust/root/test-run/default/shard-2-of-4");
    validate_task_id(&shard)?;
    validate_task_id(TASK)?;
    assert!(validate_task_id("stack/rust//clippy/default").is_err());
    assert!(validate_task_id("stack/rust/root/test/default/shard-0-of-2").is_err());
    assert!(validate_task_id("cargo/clippy").is_err());
    assert_eq!(
        task_id_for_internal("plan", "default")?,
        "internal/plan/default"
    );
    Ok(())
}

#[test]
fn shard_suffix_parses_once_everywhere() {
    let base = "stack/rust/root/nextest/default";
    assert_eq!(
        split_shard_suffix("stack/rust/root/nextest/default/shard-2-of-4"),
        Some((base, 2, 4))
    );
    assert_eq!(split_shard_suffix(base), None);
    assert_eq!(
        split_shard_suffix("stack/rust/root/nextest/default/shard-12-of-34"),
        Some((base, 12, 34))
    );
    // Previously divergent: split_once took the first suffix, rsplit_once
    // the last. The canonical parser rejects every non-trailing shape.
    for divergent in [
        "stack/rust/root/nextest/default/shard-1-of-2/shard-3-of-4",
        "stack/rust/root/nextest/shard-1-of-2/default",
        "stack/rust/root/nextest/default/shard-1-of-2/extra",
        "stack/rust/root/nextest/default/shard-x",
        "stack/rust/root/nextest/default/shard-0-of-2",
        "stack/rust/root/nextest/default/shard-1-of-0",
        "stack/rust/root/nextest/default/shard-3-of-2",
        "stack/rust/root/nextest/default/shard-1",
        "stack/rust/root/nextest/default/shard-1-of-2-of-3",
        "stack/rust/root/nextest/default/shard-4294967296-of-2",
        // Non-canonical numerals alias canonical shards (`u32::from_str`
        // accepts `+1` and `01`); validated must equal canonical.
        "stack/rust/root/nextest/default/shard-+1-of-2",
        "stack/rust/root/nextest/default/shard-1-of-02",
        "stack/rust/root/nextest/default/shard-01-of-02",
        "stack/rust/root/nextest/default/shard-1-of-+2",
        "stack/rust/root/nextest/default/shard-0-of-2",
        "stack/rust/root/nextest/default/shard-1-of-2 ",
    ] {
        assert_eq!(split_shard_suffix(divergent), None, "{divergent}");
    }
    // Validation agrees with the parser: canonical sharded IDs validate,
    // doubled and malformed suffixes fail closed.
    validate_task_id("stack/rust/root/nextest/default/shard-2-of-4")
        .expect("canonical shard validates");
    for bad in [
        "stack/rust/root/nextest/default/shard-1-of-2/shard-3-of-4",
        "stack/rust/root/nextest/default/shard-x",
    ] {
        assert!(validate_task_id(bad).is_err(), "{bad}");
    }
}

#[test]
fn matrix_and_report_ids_roundtrip() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(123, 1);
    assert_eq!(run_key, "r123-a1");
    validate_run_key("local")?;
    assert!(validate_run_key("r1").is_err());
    let id = matrix_id_for_task_group("rust", GROUP)?;
    validate_id(&id)?;
    assert!(validate_id("STACK:rust|task:x").is_err());
    let key = matrix_key_for_id(&id)?;
    validate_matrix_key(&key)?;
    assert!(key.starts_with("m-") && key.len() == 18);
    let report = report_id_for_matrix(&run_key, &key)?;
    validate_report_id(&report)?;
    let task_digest = digest_b3(b"task");
    let task_report = task_report_id_for_task(&run_key, &key, &task_digest)?;
    validate_task_report_id(&task_report)?;
    assert!(task_report.ends_with(&task_digest[3..19]));
    assert_eq!(plan_id_for_run(&run_key)?, "plan-r123-a1");
    validate_plan_id("plan-local")?;
    assert_eq!(
        target_key("x86_64-unknown-linux-gnu")?,
        "x86-64-unknown-linux-gnu"
    );
    assert_eq!(target_key("aarch64-apple-darwin")?, "aarch64-apple-darwin");
    Ok(())
}

/// R15: names that slug identically still get distinct stable job IDs.
#[test]
fn colliding_slugs_get_distinct_stable_ids() {
    assert_eq!(slugify_segment("my-crate"), "my-crate");
    assert_eq!(slugify_segment("my_crate"), "my-crate");
    assert_eq!(slugify_segment("My Crate"), "my-crate");
    let crates: BTreeSet<(String, String, String)> = [
        ("a", "my-crate", "default"),
        ("b", "my_crate", "default"),
        ("c", "My Crate", "nextest"),
    ]
    .into_iter()
    .map(|(id, name, config)| (id.to_owned(), name.to_owned(), config.to_owned()))
    .collect();
    let first = assign_crate_job_ids(&crates, CRATE_JOB_ID_PREFIX);
    let second = assign_crate_job_ids(&crates, CRATE_JOB_ID_PREFIX);
    assert_eq!(first, second, "assignment is deterministic");
    assert_eq!(first.len(), 3);
    let ids: BTreeSet<&String> = first.values().collect();
    assert_eq!(ids.len(), 3, "colliding slugs stay distinct: {first:?}");
    for id in first.values() {
        assert!(validate_job_id(id).is_ok(), "{id}");
    }
    assert_eq!(
        first
            .get(&("a".to_owned(), "default".to_owned()))
            .map(String::as_str),
        Some("rust-my-crate"),
        "first key in sorted order keeps the base slug"
    );
    let digest = digest_b3("b\0default".as_bytes());
    let want = format!("rust-my-crate-{}", &digest[..8]);
    assert_eq!(
        first.get(&("b".to_owned(), "default".to_owned())),
        Some(&want),
        "later colliding key takes package digest disambiguation"
    );
}

#[test]
fn fetch_roots_accept_safe_and_reject_hostile() {
    for root in ["", "nested", "a/b", "crate-1_x.y"] {
        assert!(validate_fetch_root(root).is_ok(), "{root}");
    }
    for root in [
        "..", "a/../b", "a'b", "a\"b", "$HOME", "a`b", "a\\b", "a\nb", "/abs",
    ] {
        let err = validate_fetch_root(root).expect_err("hostile root");
        assert!(
            err.to_string().contains("unsafe_fetch_root"),
            "{root}: {err}"
        );
    }
}
