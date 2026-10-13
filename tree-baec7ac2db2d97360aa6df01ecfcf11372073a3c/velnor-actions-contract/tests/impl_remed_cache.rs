//! Remediation cases: CACHE audit rows.
use crate::impl_contract_ids::{TASK, sample_entry, sample_identity};
use velnor_actions_contract::cachekey::{
    CACHE_SCHEMA_ID, CacheIdentity, FormatInputs, LaneInputs, MAX_CACHE_KEY_BYTES, PlatformInputs,
    ToolchainInputs, WorkspaceInputs, cache_format_id, cache_key, lane_id, platform_id,
    restore_prefix, toolchain_id, workspace_id,
};
use velnor_actions_contract::{
    ContractError, canonical_json_bytes, canonical_json_str, digest_b3, input_digest,
    is_secret_env_name, parse_strict_json, run_key_for_ci, task_report_id_for_task,
};
use velnor_actions_contract_workflow::{
    CacheLayer, CacheOutcome, CacheResult, EntryCacheIds, TaskReport, TaskStatus, Trust,
    WorkflowEvent,
};

#[test]
fn cache_secret_env_never_enters_identity() {
    assert!(is_secret_env_name("GH_TOKEN"));
    assert!(is_secret_env_name("GITHUB_TOKEN"));
    assert!(is_secret_env_name("ACTIONS_RUNTIME_TOKEN"));
    assert!(is_secret_env_name("CARGO_REGISTRY_TOKEN"));
    assert!(is_secret_env_name("api_secret"));
    assert!(is_secret_env_name("DB_PASSWORD"));
    assert!(!is_secret_env_name("RUSTFLAGS"));
    assert!(!is_secret_env_name("CARGO_BUILD_JOBS"));
    let mut identity = sample_identity();
    identity
        .environment
        .insert("GH_TOKEN".to_owned(), "x".to_owned());
    let err = identity.validate().expect_err("secret env");
    assert_eq!(
        err,
        ContractError::identity("environment", "secret_env:GH_TOKEN")
    );
    assert!(input_digest(&identity).is_err());
    assert!(sample_identity().validate().is_ok());
}

#[test]
fn cache_strict_json_rejects_duplicate_keys() {
    let dup = r#"{"a":1,"b":{"x":1,"x":2}}"#;
    let err = parse_strict_json(dup).expect_err("nested dup");
    assert!(err.to_string().contains("duplicate_key:x"), "got {err}");
    let top = r#"{"schema":1,"schema":2}"#;
    assert!(parse_strict_json(top).is_err());
    let escaped = "{\"\\u0061\":1,\"a\":2}";
    assert!(parse_strict_json(escaped).is_err());
    let ok = r#"{"b":1,"a":[3,{"z":null}]}"#;
    assert!(parse_strict_json(ok).is_ok());
    assert!(parse_strict_json("not json").is_err());
    assert!(parse_strict_json(r#"{"a":01}"#).is_err());
    let identity_text = serde_json::to_string(&sample_identity()).expect("serialize");
    let back = velnor_actions_contract::TaskIdentity::parse_json(&identity_text);
    assert!(back.is_ok());
}

#[test]
fn cache_identity_validates_exact_twelve_fields() {
    let good = CacheIdentity {
        schema_id: CACHE_SCHEMA_ID.to_owned(),
        stack_id: "rust".to_owned(),
        repository_id: digest_b3(b"repo"),
        project_root: "crates/demo".to_owned(),
        component_id: "pkg".to_owned(),
        workspace_id: digest_b3(b"w"),
        lane_id: digest_b3(b"lane"),
        platform_id: digest_b3(b"plat"),
        toolchain_id: digest_b3(b"tool"),
        cache_format_id: digest_b3(b"fmt"),
        stack_extension_id: digest_b3(b"ext"),
        input_digest: digest_b3(b"inputs"),
    };
    assert_eq!(good.validate(), Ok(()));
    let keys: Vec<String> = serde_json::to_value(&good)
        .expect("value")
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    assert_eq!(keys.len(), 12);
    let mut bad = good.clone();
    bad.schema_id = "v2".to_owned();
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.input_digest = "b3-xyz".to_owned();
    assert!(bad.validate().is_err());
    let mut bad = good;
    bad.project_root = "/abs".to_owned();
    assert!(bad.validate().is_err());
    let mut traversal = CacheIdentity {
        project_root: "a/../b".to_owned(),
        ..bad
    };
    assert_eq!(
        traversal.validate().expect_err("parent traversal"),
        ContractError::identity("path", "parent_traversal")
    );
    traversal.project_root = ".".to_owned();
    assert_eq!(traversal.validate(), Ok(()));
}

#[test]
fn workspace_project_root_rejects_traversal_and_keeps_canonical_digest() -> Result<(), ContractError>
{
    let mut workspace = WorkspaceInputs {
        repository_id: digest_b3(b"repo"),
        stack_id: "rust".to_owned(),
        project_root: ".".to_owned(),
        inventory_digest: digest_b3(b"inventory"),
    };
    let prior_canonical_digest = digest_b3(&canonical_json_bytes(&workspace)?);
    assert_eq!(workspace_id(&workspace)?, prior_canonical_digest);

    workspace.project_root = "a/../b".to_owned();
    assert_eq!(
        workspace_id(&workspace).expect_err("parent traversal"),
        ContractError::identity("path", "parent_traversal")
    );
    Ok(())
}

#[test]
fn cache_toolchain_excludes_tool_files() -> Result<(), ContractError> {
    let inputs = ToolchainInputs {
        tools: vec!["cargo-nextest@0.9.96".to_owned(), "rust@1.98.1".to_owned()],
        components: vec!["clippy".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let keys: Vec<String> = serde_json::to_value(&inputs)
        .expect("value")
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        keys,
        ["compile_driver", "components", "test_runner", "tools"]
    );
    let id = toolchain_id(&inputs)?;
    assert_eq!(id, toolchain_id(&inputs)?);
    let mut unsorted = inputs.clone();
    unsorted.tools.reverse();
    assert!(toolchain_id(&unsorted).is_err());
    let empty = ToolchainInputs {
        tools: vec![],
        ..inputs
    };
    assert!(toolchain_id(&empty).is_err());
    Ok(())
}

#[test]
fn cache_entry_records_five_identity_digests() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(21, 1);
    let mut entry = sample_entry(&run_key)?;
    assert!(entry.cache_ids.is_none());
    entry.validate(&run_key)?;
    entry.cache_ids = Some(EntryCacheIds::new(
        &digest_b3(b"w"),
        &digest_b3(b"lane"),
        &digest_b3(b"plat"),
        &digest_b3(b"tool"),
        &digest_b3(b"fmt"),
    )?);
    entry.validate(&run_key)?;
    let text = serde_json::to_string(&entry).expect("serialize");
    assert!(text.contains("cache_format_id"));
    let bad = EntryCacheIds::new(
        &digest_b3(b"w"),
        "nope",
        &digest_b3(b"plat"),
        &digest_b3(b"tool"),
        &digest_b3(b"fmt"),
    );
    assert!(bad.is_err());
    Ok(())
}

#[test]
fn cache_trust_stays_out_of_input_digest() -> Result<(), ContractError> {
    let preimage = canonical_json_str(&sample_identity())?;
    for key in ["trust", "trusted", "pr_scope", "namespace", "permission"] {
        assert!(!preimage.contains(key), "leaked {key}");
    }
    let digest = input_digest(&sample_identity())?;
    assert_eq!(digest, input_digest(&sample_identity())?);
    Ok(())
}

#[test]
fn cache_input_digest_covers_dependencies() -> Result<(), ContractError> {
    let base = input_digest(&sample_identity())?;
    let mut with_dep = sample_identity();
    with_dep.dependencies = vec!["stack/rust/root/clippy/default".to_owned()];
    let changed = input_digest(&with_dep)?;
    assert_ne!(changed, base);
    assert_eq!(input_digest(&with_dep)?, changed);
    let mut bad = sample_identity();
    bad.dependencies = vec!["not-a-task-id".to_owned()];
    assert!(bad.validate().is_err());
    let mut unsorted = sample_identity();
    unsorted.dependencies = vec![
        "stack/rust/root/test/default".to_owned(),
        "stack/rust/root/clippy/default".to_owned(),
    ];
    assert!(unsorted.validate().is_err());
    Ok(())
}

#[test]
fn cache_lane_binds_config_and_writer() -> Result<(), ContractError> {
    let lane = |config: &str, writer: &str| {
        lane_id(&LaneInputs {
            workspace_id: digest_b3(b"w"),
            component_id: "pkg".to_owned(),
            task_kind: "clippy".to_owned(),
            configuration: config.to_owned(),
            writer_lane: writer.to_owned(),
        })
    };
    let base = lane("default", "lane-0")?;
    assert_ne!(lane("nextest", "lane-0")?, base);
    assert_ne!(lane("default", "lane-1")?, base);
    assert_eq!(lane("default", "lane-0")?, base);
    Ok(())
}

#[test]
fn cache_key_shape_and_bound() -> Result<(), ContractError> {
    let compat = digest_b3(b"compat");
    let snapshot = digest_b3(b"snapshot");
    let key = cache_key("task", "trusted", &compat, &snapshot)?;
    assert!(key.len() <= MAX_CACHE_KEY_BYTES);
    assert_eq!(key, format!("velnor-v1-task-trusted-{compat}-{snapshot}"));
    for layer in ["sources", "mbx", "task"] {
        assert!(cache_key(layer, "pr", &compat, &snapshot).is_ok());
    }
    assert!(cache_key("weird", "trusted", &compat, &snapshot).is_err());
    assert!(cache_key("task", "public", &compat, &snapshot).is_err());
    assert!(cache_key("task", "trusted", "nope", &snapshot).is_err());
    let prefix = restore_prefix("mbx", "pr", &compat)?;
    assert!(prefix.ends_with('-') && !prefix.contains(&snapshot));
    let pad = "x".repeat(MAX_CACHE_KEY_BYTES);
    let overlong = format!("velnor-v1-task-trusted-{compat}-{pad}");
    assert!(overlong.len() > MAX_CACHE_KEY_BYTES);
    let run_key = run_key_for_ci(20, 1);
    let entry = sample_entry(&run_key)?;
    let task_digest = digest_b3(b"task-bytes");
    let mut report = TaskReport {
        schema: 1,
        task_report_id: task_report_id_for_task(&run_key, &entry.matrix_key, &task_digest)?,
        run_key,
        event: WorkflowEvent::PullRequest,
        trust: Trust::Pr,
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        task_id: TASK.to_owned(),
        task_digest,
        status: TaskStatus::Executed,
        not_selected_reason: None,
        cache: CacheOutcome {
            layer: CacheLayer::Task,
            key,
            result: CacheResult::Hit,
            miss_reason: None,
        },
        exit_code: 0,
        duration_ms: Some(1),
        outputs: vec![],
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    };
    report.validate()?;
    report.cache.key = overlong;
    let err = report.validate().expect_err("overlong key");
    assert_eq!(err, ContractError::identity("cache.key", "key_too_long"));
    assert!(
        workspace_id(&WorkspaceInputs {
            repository_id: digest_b3(b"repo"),
            stack_id: "rust".to_owned(),
            project_root: "crates/demo".to_owned(),
            inventory_digest: digest_b3(b"inv"),
        })
        .is_ok()
    );
    assert!(
        platform_id(&PlatformInputs {
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            image_os: "ubuntu26".to_owned(),
            image_version: "20260928.1.0".to_owned(),
            target: "host".to_owned(),
        })
        .is_ok()
    );
    assert!(
        cache_format_id(&FormatInputs {
            adapter: "cargo".to_owned(),
            format: "cargo-1".to_owned(),
            generation: "7".to_owned(),
        })
        .is_ok()
    );
    Ok(())
}

#[test]
fn cache_declared_env_value_change_invalidates_identity() -> Result<(), ContractError> {
    let base = sample_identity();
    let before = input_digest(&base)?;
    let mut changed = base.clone();
    changed.environment.insert(
        "RUSTFLAGS".to_owned(),
        "-D warnings -W clippy::pedantic".to_owned(),
    );
    changed.validate()?;
    assert_ne!(input_digest(&changed)?, before);
    Ok(())
}

#[test]
fn cache_tofu_providers_layer_builds_keys_and_prefixes() -> Result<(), ContractError> {
    let compat = digest_b3(b"compat");
    let snapshot = digest_b3(b"snapshot");
    let key = cache_key("tofu-providers", "trusted", &compat, &snapshot)?;
    assert!(key.len() <= MAX_CACHE_KEY_BYTES);
    assert_eq!(
        key,
        format!("velnor-v1-tofu-providers-trusted-{compat}-{snapshot}")
    );
    let prefix = restore_prefix("tofu-providers", "pr", &compat)?;
    assert!(prefix.ends_with('-') && !prefix.contains(&snapshot));
    Ok(())
}

#[test]
fn cache_mbx_hit_can_never_satisfy_task_obligation() -> Result<(), ContractError> {
    let compat = digest_b3(b"compat");
    let snapshot = digest_b3(b"snapshot");
    let mbx_key = cache_key("mbx", "pr", &compat, &snapshot)?;
    let task_key = cache_key("task", "pr", &compat, &snapshot)?;
    assert_ne!(mbx_key, task_key, "layer is part of key identity");
    let mbx_prefix = restore_prefix("mbx", "pr", &compat)?;
    assert!(
        !task_key.starts_with(&mbx_prefix),
        "mbx transport cannot feed the task gate"
    );
    Ok(())
}
