use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::{HelperInvocation, SourceBoundHelper};
use velnor_actions_contract::{StepId, ToolCacheDescriptor, ToolProducerSelection};

fn fixture_record() -> CompiledSourceHelper {
    let selectors = vec!["gh@2.87.3".to_owned()];
    let qualification = format!("qualified-tools@b3-{}", "a".repeat(64));
    let env = BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("RUSTUP_AUTO_INSTALL".to_owned(), "0".to_owned()),
        ("VELNOR_MISE_SHA256".to_owned(), "a".repeat(64)),
        (
            "MISE_DATA_DIR".to_owned(),
            ToolCacheDomain::Planning.root().to_owned(),
        ),
        (
            "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
            qualification.clone(),
        ),
        (
            "VELNOR_TOOL_CACHE_IDENTITY".to_owned(),
            format!(
                "toolset@{}",
                velnor_actions_contract::digest_b3(selectors.join("\0").as_bytes())
            ),
        ),
    ]);
    let source = crate::marker::with_marker("0.1.0", "exit 0\n").expect("source");
    let operation = SourceBoundOperation::MiseToolPrepare;
    let invocation = HelperInvocation::compiled(
        SourceBoundHelper::compiled(
            operation,
            operation.path(),
            &velnor_actions_contract::compiled_source_sha256(source.as_bytes()),
        )
        .expect("descriptor"),
        vec!["planning".to_owned(), selectors[0].clone()],
        selectors.clone(),
    )
    .expect("invocation");
    CompiledSourceHelper::compiled(invocation, source)
        .expect("record")
        .with_environment(env)
}

fn fixture() -> (Job, MiseSetup, Vec<CompiledSourceHelper>) {
    let setup = crate::setup::fixture::mise_setup("2026.9.18", &"a".repeat(64));
    let record = fixture_record();
    let install = crate::source_helper::source_helper_step(
        "Prepare and verify executable cache",
        &record,
        record.environment().clone(),
    )
    .expect("install");
    let mut job: Job = serde_json::from_value(serde_json::json!({
        "display_name": "Pure planning tools", "runs_on": "ubuntu-24.04",
        "timeout_minutes": 10, "needs": [], "steps": [install]
    }))
    .expect("job");
    let specs = crate::cache_p08::infer_job_tools(&job);
    let key = crate::cache_p08::mise_cache_key_for_tools(
        "x86_64-unknown-linux-gnu",
        &setup.version,
        &specs,
    )
    .expect("key")
    .replacen("mise-v3-", "mise-v3-planning-", 1);
    let id = |value: &str| StepId::new(value).expect("id");
    let meta = PureToolProducer {
        descriptor: ToolCacheDescriptor {
            domain: ToolCacheDomain::Planning,
            target: "x86_64-unknown-linux-gnu".to_owned(),
            runs_on: job.runs_on.clone(),
            selectors: record.invocation().installed_selectors().to_vec(),
            immutable_identity: key,
            qualification_identity: record.environment()["VELNOR_QUALIFIED_TOOL_IDENTITY"].clone(),
        },
        selection: ToolProducerSelection::default(),
        restore_step: id("velnor-planning-tools-cache"),
        before_step: id("velnor-tool-before"),
        installation_step: id("velnor-tool-install"),
        after_step: id("velnor-tool-after"),
        save_step: id("velnor-tool-save"),
        report_step: id("velnor-tool-report"),
    };
    job.outputs = producer_outputs(&meta);
    job.cache_mode = Some(velnor_actions_contract::CacheMode::Write);
    job.condition = Some(meta.selection.condition(meta.descriptor.domain));
    job.permissions = Some(Permissions {
        contents: PermissionLevel::None,
        actions: PermissionLevel::None,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        issues: PermissionLevel::None,
        pages: PermissionLevel::None,
        attestations: PermissionLevel::None,
    });
    let before = snapshot_fixture(true);
    let after = snapshot_fixture(false);
    job.steps = producer_steps(&meta, &setup, &record, &before, &after, "0.1.0").expect("steps");
    let records = vec![
        before,
        after,
        record,
        report_record(&meta, "0.1.0").expect("report"),
        setup
            .bootstrap(meta.descriptor.domain, &job.runs_on)
            .expect("bootstrap")
            .helper
            .clone(),
    ];
    job.tool_producer = Some(meta);
    (job, setup, records)
}

fn snapshot_fixture(before: bool) -> CompiledSourceHelper {
    let domain = CacheSnapshotDomain::PlanningTools;
    let phase = if before { "before" } else { "after" };
    let source = crate::marker::with_marker("0.1.0", "exit 0\n").expect("source");
    let operation = SourceBoundOperation::CacheSnapshot;
    let helper = SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &velnor_actions_contract::compiled_source_sha256(source.as_bytes()),
    )
    .expect("descriptor");
    let invocation = HelperInvocation::compiled(
        helper,
        vec![domain.name().to_owned(), phase.to_owned()],
        Vec::new(),
    )
    .expect("invocation");
    let environment = BTreeMap::from([
        ("VELNOR_SNAPSHOT_LAYER".to_owned(), domain.name().to_owned()),
        ("VELNOR_SNAPSHOT_PHASE".to_owned(), phase.to_owned()),
        ("VELNOR_SNAPSHOT_ROOTS".to_owned(), domain.roots().join(",")),
        (
            "VELNOR_SNAPSHOT_OUTPUT".to_owned(),
            domain.name().to_ascii_uppercase(),
        ),
        (
            "VELNOR_SNAPSHOT_RESTORED".to_owned(),
            format!(
                "${{{{steps.{}.outputs.cache-matched-key}}}}",
                domain.restore_id()
            ),
        ),
    ]);
    CompiledSourceHelper::compiled(invocation, source)
        .expect("record")
        .with_environment(environment)
}

#[test]
fn canonical_pure_sequence_is_admitted() {
    let (job, setup, records) = fixture();
    assert!(validate_tool_producer(&job, &setup, &records).expect("admitted"));
    let meta = job.tool_producer.as_ref().expect("metadata");
    assert!(meta.save_condition().contains("outputs.verified == 'true'"));
    assert!(meta.save_condition().contains("outputs.changed == 'true'"));
}

#[test]
fn every_step_rejects_added_environment_authority() {
    let (original, setup, records) = fixture();
    for index in 0..original.steps.len() {
        let mut job = original.clone();
        let env = match &mut job.steps[index].kind {
            StepKind::Shell { env, .. }
            | StepKind::Action { env, .. }
            | StepKind::SourceBoundHelper { env, .. } => env,
            StepKind::Internal { .. } => panic!("pure sequence cannot contain Internal"),
        };
        env.insert("NODE_AUTH_TOKEN".to_owned(), "injected".to_owned());
        assert!(
            validate_tool_producer(&job, &setup, &records).is_err(),
            "step {index}"
        );
    }
}

#[test]
fn ordering_payload_pins_and_isolation_fail_closed() {
    let (original, setup, records) = fixture();
    for index in 0..original.steps.len() - 1 {
        let mut job = original.clone();
        job.steps.swap(index, index + 1);
        assert!(
            validate_tool_producer(&job, &setup, &records).is_err(),
            "swap {index}"
        );
    }
    for index in [1, 6, 7] {
        let mut job = original.clone();
        let StepKind::Action { with, .. } = &mut job.steps[index].kind else {
            panic!("action")
        };
        with.insert("foreign".to_owned(), "arbitrary".to_owned());
        assert!(
            validate_tool_producer(&job, &setup, &records).is_err(),
            "action {index}"
        );
    }
    for key in [
        "MISE_NO_CONFIG",
        "MISE_NO_ENV",
        "MISE_NO_HOOKS",
        "MISE_LOCKFILE",
        "MISE_AUTO_INSTALL",
        "MISE_EXEC_AUTO_INSTALL",
        "RUSTUP_AUTO_INSTALL",
    ] {
        let mut job = original.clone();
        let StepKind::SourceBoundHelper { env, .. } = &mut job.steps[4].kind else {
            panic!("helper")
        };
        env.remove(key);
        assert!(
            validate_tool_producer(&job, &setup, &records).is_err(),
            "missing {key}"
        );
    }
}

#[test]
fn repository_computation_and_private_job_authority_are_rejected() {
    let (original, setup, records) = fixture();
    let foreign = [
        StepKind::Internal {
            operation: "plan".to_owned(),
        },
        StepKind::Shell {
            run: vec!["npm".to_owned(), "install".to_owned()],
            env: BTreeMap::new(),
        },
        StepKind::Action {
            uses: "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    ];
    for kind in foreign {
        let mut job = original.clone();
        job.steps.insert(
            6,
            Step {
                id: None,
                name: "Injected".to_owned(),
                condition: None,
                kind,
            },
        );
        assert!(validate_tool_producer(&job, &setup, &records).is_err());
    }
    let mut job = original.clone();
    job.environment = Some("production".to_owned());
    assert!(validate_tool_producer(&job, &setup, &records).is_err());
    let mut job = original;
    job.permissions.as_mut().expect("permissions").id_token = PermissionLevel::Write;
    assert!(validate_tool_producer(&job, &setup, &records).is_err());
}

#[test]
fn descriptor_tampering_and_scheduling_cannot_grant_authority() {
    let (original, setup, records) = fixture();
    let mut job = original.clone();
    job.tool_producer
        .as_mut()
        .expect("metadata")
        .descriptor
        .immutable_identity
        .push_str("-arbitrary");
    assert!(validate_tool_producer(&job, &setup, &records).is_err());
    let mut job = original.clone();
    job.runs_on = "macos-15".to_owned();
    assert!(validate_tool_producer(&job, &setup, &records).is_err());
    let mut job = original.clone();
    job.condition = Some("true".to_owned());
    assert!(validate_tool_producer(&job, &setup, &records).is_err());
    let mut job = original;
    job.tool_producer = None;
    assert!(validate_tool_producer(&job, &setup, &records).is_err());
}

#[test]
fn qualified_shape_without_compiled_registry_never_executes() {
    let (job, setup, _) = fixture();
    assert!(validate_tool_producer(&job, &setup, &[]).is_err());
    for index in [2, 3, 4, 5, 8] {
        let step = &job.steps[index];
        let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
            panic!("compiled helper")
        };
        assert!(
            crate::source_helper::step_to_yaml(step, invocation, env, &[], &job.runs_on).is_err()
        );
    }
}

#[test]
fn evidence_and_report_exports_must_stay_exact() {
    let (original, setup, records) = fixture();
    for index in 0..original.steps.len() {
        let mut job = original.clone();
        job.steps[index].condition = Some("true".to_owned());
        assert!(
            validate_tool_producer(&job, &setup, &records).is_err(),
            "condition {index}"
        );
    }
    let mut job = original.clone();
    job.needs.push("unrelated".to_owned());
    assert!(validate_tool_producer(&job, &setup, &records).is_err());
    let mut job = original;
    job.outputs.pop();
    assert!(validate_tool_producer(&job, &setup, &records).is_err());
}

#[test]
fn pure_producer_requires_explicit_server_write_mode() {
    use velnor_actions_contract::CacheMode;
    let (original, setup, records) = fixture();
    for mode in [
        None,
        Some(CacheMode::Read),
        Some(CacheMode::WriteOnly),
        Some(CacheMode::None),
    ] {
        let mut job = original.clone();
        job.cache_mode = mode;
        assert!(validate_tool_producer(&job, &setup, &records).is_err());
    }
}

#[test]
fn pure_producer_requires_exact_rustup_auto_install_disable_value() {
    let (original, setup, records) = fixture();
    let meta = original.tool_producer.as_ref().expect("metadata");
    for value in [None, Some("1"), Some("false"), Some("")] {
        let mut step = original.steps[4].clone();
        let StepKind::SourceBoundHelper { env, .. } = &mut step.kind else {
            panic!("helper")
        };
        env.remove("RUSTUP_AUTO_INSTALL");
        if let Some(value) = value {
            env.insert("RUSTUP_AUTO_INSTALL".to_owned(), value.to_owned());
        }
        assert!(validate_install(&step, meta, &setup).is_err());
        let mut job = original.clone();
        job.steps[4] = step;
        assert!(validate_tool_producer(&job, &setup, &records).is_err());
    }
}

#[test]
fn publication_receipt_requires_exact_lookup_only_transport() {
    let (original, setup, records) = fixture();
    assert_eq!(original.steps.len(), 9);
    let receipt = &original.steps[7];
    assert_eq!(
        receipt.id.as_ref().expect("id").as_str(),
        "velnor-tool-publication"
    );
    let StepKind::Action { with, .. } = &receipt.kind else {
        panic!("receipt action")
    };
    assert_eq!(with.get("lookup-only").map(String::as_str), Some("true"));
    let StepKind::Action { with: save, .. } = &original.steps[6].kind else {
        panic!("save action")
    };
    assert_eq!(with.get("key"), save.get("key"));
    assert_eq!(with.get("path"), save.get("path"));
    for field in ["lookup-only", "key", "path", "restore-keys"] {
        let mut job = original.clone();
        let StepKind::Action { with, .. } = &mut job.steps[7].kind else {
            panic!("receipt action")
        };
        with.insert(field.to_owned(), "tampered".to_owned());
        assert!(
            validate_tool_producer(&job, &setup, &records).is_err(),
            "{field}"
        );
    }
}
