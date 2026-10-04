use super::*;
use crate::cache_p08::tool_roles::{producer_outputs, producer_steps, report_record};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CacheSnapshotDomain, Permissions, PureToolProducer, SourceBoundOperation, ToolCacheDomain,
};
use velnor_actions_contract::{HelperInvocation, SourceBoundHelper};
use velnor_actions_contract::{StepId, ToolCacheDescriptor, ToolProducerSelection};

pub(super) fn fixture_record() -> CompiledSourceHelper {
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

pub(super) fn fixture() -> (Job, MiseSetup, Vec<CompiledSourceHelper>) {
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
