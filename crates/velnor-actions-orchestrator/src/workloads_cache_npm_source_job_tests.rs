use super::{producer_job, restore_step, source_key};
use crate::workloads::cache_eligibility::NativeNpmSource;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    SourceBoundOperation, SourceProducerRole, Step, StepKind, ToolProducerSelection,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::MiseSetup;

const RUNNER: &str = "ubuntu-26.04";
const NODE_VERSION: &str = velnor_actions_mise::catalog::NODE_VERSION;
const INTEGRITY_A: &str = "sha512-LCpQRaCgZUZ2pSPVoon4GB+7kQmwPoFYikmLkF0FXqdAJVV3bjCZahRWiL5jPlsIm5qrqI88s4Zgjwaiu/sTlQ==";
const INTEGRITY_B: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

fn source(name: &str, version: &str, resolved: &str, integrity: &str) -> NativeNpmSource {
    NativeNpmSource {
        name: name.to_owned(),
        version: version.to_owned(),
        resolved: resolved.to_owned(),
        integrity: integrity.to_owned(),
    }
}

fn sources() -> Vec<NativeNpmSource> {
    vec![
        source(
            "@types/node",
            "22.0.0",
            "https://registry.npmjs.org/@types/node/-/node-22.0.0.tgz",
            INTEGRITY_A,
        ),
        source(
            "typescript",
            "5.6.3",
            "https://registry.npmjs.org/typescript/-/typescript-5.6.3.tgz",
            INTEGRITY_B,
        ),
    ]
}

fn setup() -> MiseSetup {
    crate::test_mise::setup("2026.9.16", &"a".repeat(64))
}

fn selection() -> ToolProducerSelection {
    ToolProducerSelection {
        tasks: vec!["stack/rust/example/test/default".to_owned()],
        cargo_fallback: false,
        unconditional: false,
    }
}

fn build_job(
    candidates: &[NativeNpmSource],
    catalog: &ToolCatalog,
    selected: &ToolProducerSelection,
) -> Result<velnor_actions_contract::Job, crate::OrchestratorError> {
    producer_job(
        candidates,
        catalog,
        RUNNER,
        &setup(),
        env!("CARGO_PKG_VERSION"),
        selected,
    )
}

fn key(source: &NativeNpmSource, runner: &str) -> String {
    source_key(std::slice::from_ref(source), &ToolCatalog::pinned(), runner).expect("source key")
}
fn action_inputs(step: &Step) -> &BTreeMap<String, String> {
    let StepKind::Action { with, .. } = &step.kind else {
        panic!("expected cache action")
    };
    with
}

fn step_env(step: &Step) -> Option<&BTreeMap<String, String>> {
    match &step.kind {
        StepKind::Action { env, .. }
        | StepKind::Shell { env, .. }
        | StepKind::SourceBoundHelper { env, .. } => Some(env),
        _ => None,
    }
}

#[test]
fn source_key_sorts_and_deduplicates_complete_source_tuples() {
    let catalog = ToolCatalog::pinned();
    let mut unordered = sources();
    let first = unordered[0].clone();
    unordered.push(first);
    unordered.reverse();
    assert_eq!(
        source_key(&sources(), &catalog, RUNNER).expect("canonical key"),
        source_key(&unordered, &catalog, RUNNER).expect("unordered key")
    );
}

#[test]
fn source_key_binds_name_version_url_integrity_and_runner() {
    let base = sources().remove(0);
    let base_key = key(&base, RUNNER);
    let changed_key = |name, version, resolved, integrity| {
        key(&source(name, version, resolved, integrity), RUNNER)
    };
    assert_ne!(
        base_key,
        changed_key(
            "@types/other",
            &base.version,
            &base.resolved,
            &base.integrity
        )
    );
    assert_ne!(
        base_key,
        changed_key(&base.name, "23.0.0", &base.resolved, &base.integrity)
    );
    assert_ne!(
        base_key,
        changed_key(
            &base.name,
            &base.version,
            "https://registry.npmjs.org/@types/node/-/other-22.0.0.tgz",
            &base.integrity
        )
    );
    assert_ne!(
        base_key,
        changed_key(&base.name, &base.version, &base.resolved, INTEGRITY_B)
    );
    assert_ne!(base_key, key(&base, "macos-26"), "changed runner");
}
#[test]
fn producer_job_rejects_empty_source_candidates() {
    assert!(
        build_job(&[], &ToolCatalog::pinned(), &selection()).is_err_and(|error| error
            .to_string()
            .contains("npm_source_producer_without_candidates"))
    );
}
#[test]
fn producer_job_has_no_checkout_repository_script_or_private_credential() {
    let job = build_job(&sources(), &ToolCatalog::pinned(), &selection()).expect("producer job");
    assert!(
        job.environment.is_none(),
        "producer has no protected environment"
    );
    for step in &job.steps {
        match &step.kind {
            StepKind::Action { uses, with, env } => {
                assert!(
                    !uses.contains("actions/checkout"),
                    "producer checks out a repository"
                );
                for value in with.values().chain(env.values()) {
                    assert!(!value.contains("github.workspace"));
                    assert!(!value.contains("runner.workspace"));
                    assert!(!value.contains("secrets."));
                }
            }
            StepKind::Shell { run, env } => {
                for argument in run {
                    for forbidden in [
                        "npm ci",
                        "npm install",
                        "npm run",
                        "npm test",
                        "npm exec",
                        "github.workspace",
                        "runner.workspace",
                    ] {
                        assert!(
                            !argument.contains(forbidden),
                            "producer invokes repository command {forbidden}"
                        );
                    }
                }
                for value in env.values() {
                    assert!(!value.contains("secrets."));
                }
            }
            StepKind::SourceBoundHelper { invocation, env } => {
                for argument in invocation.args() {
                    assert!(!argument.contains("github.workspace"));
                    assert!(!argument.contains("runner.workspace"));
                    assert!(!argument.contains("secrets."));
                }
                for value in env.values() {
                    assert!(!value.contains("secrets."));
                }
            }
            _ => {}
        }
        if let Some(env) = step_env(step) {
            for (name, value) in env {
                if name.ends_with("_TOKEN")
                    || name.contains("AUTH")
                    || name.contains("PASSWORD")
                    || name.contains("SECRET")
                {
                    assert!(
                        value.is_empty() || value == "/dev/null",
                        "credential-shaped env value in {name}"
                    );
                }
            }
        }
    }
}
#[test]
fn producer_restore_uses_its_compatibility_prefix_but_consumer_restore_does_not() {
    let candidates = sources();
    let catalog = ToolCatalog::pinned();
    let key = source_key(&candidates, &catalog, RUNNER).expect("source key");
    let compatible = super::compatibility_prefix(&catalog, RUNNER).expect("compatible prefix");
    let other_runner =
        super::compatibility_prefix(&catalog, "macos-26").expect("other compatible prefix");
    assert_ne!(compatible, other_runner);
    let job = build_job(&candidates, &catalog, &selection()).expect("producer job");
    let restore = job
        .steps
        .iter()
        .find(|step| {
            step.id
                .as_ref()
                .is_some_and(|id| id.as_str() == "velnor-npm-cache")
        })
        .expect("producer restore");
    let producer_inputs = action_inputs(restore);
    assert_eq!(
        producer_inputs["restore-keys"],
        format!("{key}-snapshot-\n{compatible}")
    );
    let consumer = restore_step(&key).expect("consumer restore");
    let consumer_inputs = action_inputs(&consumer);
    assert_eq!(consumer_inputs["restore-keys"], format!("{key}-snapshot-"));
    assert!(!consumer_inputs["restore-keys"].contains(&compatible));
}
#[test]
fn producer_restores_before_pinned_node_helper() {
    let candidates = sources();
    let catalog = ToolCatalog::pinned();
    let key = source_key(&candidates, &catalog, RUNNER).expect("source key");
    let job = build_job(&candidates, &catalog, &selection()).expect("producer job");
    let restore_at = job
        .steps
        .iter()
        .position(|step| {
            step.id
                .as_ref()
                .is_some_and(|id| id.as_str() == "velnor-npm-cache")
        })
        .expect("source restore");
    let node = format!(
        "${{{{ runner.temp }}}}/velnor/npm-source/mise/installs/node/{NODE_VERSION}/bin/node"
    );
    let helper_at = job
        .steps
        .iter()
        .position(|step| {
            step.id
                .as_ref()
                .is_some_and(|id| id.as_str() == "velnor-npm-public-proof")
        })
        .expect("pinned node source helper");
    assert!(restore_at < helper_at, "restore must precede source helper");
    let StepKind::SourceBoundHelper { invocation, env } = &job.steps[helper_at].kind else {
        panic!("source helper");
    };
    assert!(!env.contains_key("VELNOR_NPM_NODE_BIN"));
    assert_eq!(invocation.args().first(), Some(&node));
    assert_eq!(invocation.args().get(1), Some(&key));
    let body_sha = &invocation.args()[2];
    assert_eq!(body_sha, invocation.descriptor().source_sha256());
    let compatibility = super::super::npm_proof::source_record(
        &[],
        &catalog,
        velnor_actions_mise::catalog::qualification::DistributionHost::LinuxAmd64,
        &node,
        "compatibility-schema",
        env!("CARGO_PKG_VERSION"),
    )
    .expect("compatibility proof record");
    assert_eq!(
        compatibility.invocation().descriptor().source_sha256(),
        body_sha
    );
    assert!(body_sha.len() == 64 && body_sha.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(
        invocation.args()[3],
        serde_json::to_string(&super::super::npm_proof::owner_expectation(&catalog))
            .expect("owner expectation")
    );
    let mut expected_descriptors = candidates.clone();
    expected_descriptors.sort();
    expected_descriptors.dedup();
    let descriptors = invocation.args()[4..]
        .iter()
        .map(|argument| {
            serde_json::from_str::<NativeNpmSource>(argument).expect("source descriptor argument")
        })
        .collect::<Vec<_>>();
    assert_eq!(descriptors, expected_descriptors);
}
#[test]
fn producer_has_one_scoped_save_after_source_proof() {
    let candidates = sources();
    let catalog = ToolCatalog::pinned();
    let key = source_key(&candidates, &catalog, RUNNER).expect("source key");
    let job = build_job(&candidates, &catalog, &selection()).expect("producer job");
    let saves: Vec<_> = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| match &step.kind {
            StepKind::Action { uses, .. }
                if uses == velnor_actions_workflow_renderer::steps::TOOLS_SAVE_USES =>
            {
                Some((index, step))
            }
            _ => None,
        })
        .collect();
    assert_eq!(saves.len(), 1, "only the producer may save");
    let (save_at, save) = saves[0];
    assert_eq!(
        save_at,
        job.steps.len() - 3,
        "publication receipt and report follow save"
    );
    let save_inputs = action_inputs(save);
    let expected_payload = super::super::npm::payload_paths().join("\n");
    assert_eq!(save_inputs["path"], expected_payload);
    let expected_key = format!(
        "{key}-snapshot-{}-{}-{}",
        "${{env.VELNOR_NPM_DOWNLOADS_SNAPSHOT_DIGEST}}",
        "${{github.run_id}}",
        "${{github.run_attempt}}",
    );
    assert_eq!(save_inputs["key"], expected_key);
    assert!(!save_inputs["key"].contains("github.sha"));
    let condition = save.condition.as_deref().expect("save gate");
    assert!(condition.contains("steps.velnor-npm-public-proof.outputs.verified == 'true'"));
}
#[test]
fn producer_registers_authority_records_and_reports_after_save() {
    let candidates = sources();
    let catalog = ToolCatalog::pinned();
    let key = source_key(&candidates, &catalog, RUNNER).expect("source key");
    let selected = selection();
    let job = build_job(&candidates, &catalog, &selected).expect("producer job");
    let metadata = job.source_producer.as_ref().expect("source metadata");
    assert_eq!(metadata.role, SourceProducerRole::Npm);
    assert_eq!(metadata.selection, selected);
    assert_eq!(metadata.source_identity, key);
    let report = job.steps.last().expect("terminal source report");
    assert_eq!(
        report.id.as_ref().expect("report ID").as_str(),
        "velnor-source-report"
    );
    assert_eq!(report.condition.as_deref(), Some("always()"));
    let StepKind::SourceBoundHelper { invocation, env } = &report.kind else {
        panic!("source report helper");
    };
    assert_eq!(
        invocation.descriptor().operation(),
        SourceBoundOperation::SourceProducerReport
    );
    assert_eq!(env["VELNOR_SOURCE_IDENTITY"], key);
    assert_eq!(
        env["VELNOR_SOURCE_VERIFIED"],
        "${{ steps.velnor-npm-public-proof.outputs.verified }}"
    );
    assert_eq!(
        env["VELNOR_SOURCE_ERROR"],
        "${{ steps.velnor-npm-public-proof.outputs.error }}"
    );
    for name in [
        "VELNOR_SOURCE_OUTCOME",
        "VELNOR_SOURCE_RESTORE_OUTCOME",
        "VELNOR_SOURCE_SAVE_OUTCOME",
    ] {
        assert!(env.contains_key(name), "missing report binding {name}");
    }
    registry_tests::assert_authority_records(&job, &candidates, &catalog, &selected);
}

#[path = "workloads_cache_npm_source_registry_tests.rs"]
mod registry_tests;

#[path = "workloads_cache_npm_source_receipt_tests.rs"]
mod receipt_tests;
