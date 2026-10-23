use super::*;
use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

#[test]
fn evidence_collector_isolated_fetch_then_verified_local_consumer() -> TestResult {
    let rendered = render_mise_task_suites(&spec(evidence_suites()))?;
    let yaml = &rendered[0].bytes;
    let fetch = yaml
        .split("  mise-task-collector:")
        .next()
        .ok_or("missing consumer job")?;
    let consumer = yaml
        .split("  mise-task-collector:")
        .nth(1)
        .ok_or("missing consumer job")?;
    assert!(fetch.contains("github-evidence-fetch-collector:"));
    assert!(fetch.contains("actions: read"));
    assert!(fetch.contains("contents: read"));
    assert!(fetch.contains("GH_TOKEN: ${{ github.token }}"));
    assert!(fetch.contains("VELNOR_INTERNAL_OP: github-evidence-fetch-v1"));
    assert!(fetch.contains("Upload GitHub evidence snapshot"));
    assert!(!fetch.contains("Checkout event commit"));
    assert!(!fetch.contains("mise-action"));
    assert!(!fetch.contains("Run declared Mise task suite"));
    assert!(consumer.contains("needs:") && consumer.contains("github-evidence-fetch-collector"));
    let download = consumer.find("Download candidate").ok_or("download")?;
    let import = consumer.find("github-evidence-import-v1").ok_or("import")?;
    let task = consumer
        .find("Run declared Mise task suite")
        .ok_or("task")?;
    assert!(download < import && import < task);
    assert!(!consumer.contains("GH_TOKEN: ${{ github.token }}"));
    assert!(!yaml.contains("concurrency:"));
    Ok(())
}

#[test]
fn evidence_collector_rejects_partial_renderer_policy() {
    let mut suites = evidence_suites();
    let mut collector = suites.remove(0);
    collector.evidence_helper = None;
    suites.insert(0, collector);
    assert!(render_mise_task_suites(&spec(suites)).is_err());
}

#[test]
fn evidence_policy_requires_snapshot_input_and_complete_release_helper() {
    let mut suites = evidence_suites();
    let mut collector = suites.remove(0);
    collector.suite.evidence_snapshot = None;
    suites.insert(0, collector);
    assert!(render_mise_task_suites(&spec(suites)).is_err());

    let mut suites = evidence_suites();
    let mut collector = suites.remove(0);
    collector.evidence_fetch_spec = None;
    collector.evidence_helper = None;
    suites.insert(0, collector);
    assert!(render_mise_task_suites(&spec(suites)).is_err());
}

#[test]
fn evidence_helper_rejects_wrong_version_target_and_release_asset_binding() {
    let version = env!("CARGO_PKG_VERSION");
    let mut manifest = evidence_helper_manifest();
    assert!(
        EvidenceHelperProvenance::from_release_manifest(&manifest, "ubuntu-26.04", "9.9.9")
            .is_err()
    );
    assert!(
        EvidenceHelperProvenance::from_release_manifest(&manifest, "custom-runner", version)
            .is_err()
    );
    manifest.targets[0].artifact = "https://example.invalid/velnor-actions".to_owned();
    assert!(
        EvidenceHelperProvenance::from_release_manifest(&manifest, "ubuntu-26.04", version)
            .is_err()
    );
}

#[test]
fn evidence_fetch_step_inventory_rejects_arbitrary_step_injection() -> TestResult {
    let suites = evidence_suites();
    let render_spec = spec(suites.clone());
    let collector = &suites[0];
    let expected = evidence::fixed_fetch_steps(collector, &render_spec)?;
    evidence::validate_fetch_step_inventory(collector, &render_spec, &expected)?;
    reject_fetch_step_mutations(collector, &render_spec, &expected)
}

fn reject_fetch_step_mutations(
    collector: &MiseTaskSuitePolicy,
    render_spec: &MiseTaskSuitesRenderSpec,
    expected: &[Step],
) -> TestResult {
    let mut mutations: Vec<Vec<Step>> = Vec::new();
    let mut checkout = expected.to_vec();
    checkout.insert(
        0,
        Step {
            name: "Checkout event commit".to_owned(),
            id: None,
            role: None,
            condition: None,
            parallel: false,
            kind: StepKind::Action {
                uses: crate::steps::CHECKOUT_USES.to_owned(),
                with: BTreeMap::from([("ref".to_owned(), "${{ github.sha }}".to_owned())]),
                env: BTreeMap::new(),
            },
        },
    );
    mutations.push(checkout);
    let mut shell = expected.to_vec();
    shell.insert(
        0,
        crate::steps::shell_step("Injected command", vec!["true".to_owned()], BTreeMap::new())?,
    );
    mutations.push(shell);
    let mut mise = expected.to_vec();
    mise.insert(
        0,
        crate::steps::action_step(
            "Injected Mise",
            "jdx/mise-action@0123456789012345678901234567890123456789",
            BTreeMap::new(),
        )?,
    );
    mutations.push(mise);
    let mut local_action = expected.to_vec();
    local_action.insert(
        0,
        Step {
            name: "Injected local action".to_owned(),
            id: None,
            role: None,
            condition: None,
            parallel: false,
            kind: StepKind::Action {
                uses: "./.github/actions/injected".to_owned(),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        },
    );
    mutations.push(local_action);
    let acquire_mutations: [fn(&mut Step); 6] = [
        |step: &mut Step| step.id = Some(velnor_actions_contract::StepId::Plan),
        |step: &mut Step| step.condition = Some("always()".to_owned()),
        |step: &mut Step| step.parallel = true,
        |step: &mut Step| step.role = None,
        |step: &mut Step| {
            if let StepKind::Shell { env, .. } = &mut step.kind {
                env.insert("GITHUB_TOKEN".to_owned(), "${{ github.token }}".to_owned());
            }
        },
        |step: &mut Step| {
            if let StepKind::Shell { run, .. } = &mut step.kind {
                run[2].push_str("; injected");
            }
        },
    ];
    for mutate in acquire_mutations {
        let mut changed = expected.to_vec();
        mutate(&mut changed[0]);
        mutations.push(changed);
    }
    assert!(mutations.iter().all(|candidate| {
        evidence::validate_fetch_step_inventory(collector, render_spec, candidate).is_err()
    }));
    Ok(())
}
