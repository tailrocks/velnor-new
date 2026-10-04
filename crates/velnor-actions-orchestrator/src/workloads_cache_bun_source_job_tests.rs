use super::*;

fn candidates() -> Vec<NativeNpmSource> {
    vec![NativeNpmSource {
        name: "is-number".to_owned(), version: "7.0.0".to_owned(),
        resolved: "https://registry.npmjs.org/is-number/-/is-number-7.0.0.tgz".to_owned(),
        integrity: "sha512-41Cifkg6e8TylSpdtTpeLVMqvSBEVzTttHvERD741+pnZ8ANv0004MRL43QKPDlK9cGvNp6NZWZUBlbGXYxxng==".to_owned(),
    }]
}

fn selection() -> ToolProducerSelection {
    ToolProducerSelection {
        tasks: vec!["stack/workload/package/bun_ci".to_owned()],
        cargo_fallback: false,
        unconditional: false,
    }
}

fn mise() -> MiseSetup {
    crate::test_mise::setup("2026.9.16", &"a".repeat(64))
}

#[test]
fn pure_job_rejects_empty_and_registers_all_exact_source_records() {
    let catalog = ToolCatalog::pinned();
    assert!(
        producer_job(
            &[],
            &catalog,
            "ubuntu-26.04",
            &mise(),
            env!("CARGO_PKG_VERSION"),
            &selection()
        )
        .is_err()
    );
    let job = producer_job(
        &candidates(),
        &catalog,
        "ubuntu-26.04",
        &mise(),
        env!("CARGO_PKG_VERSION"),
        &selection(),
    )
    .expect("job");
    let records = source_records(
        &candidates(),
        &catalog,
        "ubuntu-26.04",
        &mise(),
        env!("CARGO_PKG_VERSION"),
        &selection(),
    )
    .expect("records");
    let metadata = job.source_producer.as_ref().expect("metadata");
    let descriptor = metadata.tool_cache.as_ref().expect("bound tools");
    let prefix = velnor_actions_workflow_renderer::tool_producer_steps::tool_consumer_steps(
        descriptor,
        &mise(),
    )
    .expect("canonical prefix");
    assert_eq!(prefix.len(), 3);
    assert_eq!(job.steps[..3], prefix);
    for step in &job.steps[3..] {
        match &step.kind {
            StepKind::SourceBoundHelper { invocation, env } => {
                assert!(
                    records
                        .iter()
                        .any(|record| record.invocation() == invocation
                            && record.environment() == env)
                );
            }
            StepKind::Action { uses, env, .. } => {
                assert!(!uses.contains("checkout"));
                assert!(env.values().all(
                    |value| !value.contains("secrets.") && !value.contains("github.workspace")
                ));
            }
            _ => panic!("producer must contain only closed helpers and native cache/tool actions"),
        }
    }
    let metadata = job.source_producer.expect("metadata");
    assert_eq!(metadata.role, SourceProducerRole::Bun);
    assert_eq!(
        job.steps.last().expect("report").condition.as_deref(),
        Some("always()")
    );
}

#[test]
fn consumer_restore_stays_exact_source_while_producer_can_reuse_public_compatibility() {
    let catalog = ToolCatalog::pinned();
    let key = source_key(&candidates(), &catalog, "ubuntu-26.04").expect("key");
    let step = restore_step(&key).expect("restore");
    let StepKind::Action { with, .. } = step.kind else {
        panic!("cache action")
    };
    assert_eq!(with["path"], super::super::bun::STORE);
    assert_eq!(with["restore-keys"], format!("{key}-snapshot-"));
    assert_ne!(
        key,
        source_key(&candidates(), &catalog, "macos-26").expect("platform key")
    );
    let mut changed = candidates();
    changed[0].integrity = format!("sha512-{}==", "A".repeat(86));
    assert_ne!(
        key,
        source_key(&changed, &catalog, "ubuntu-26.04").expect("changed source")
    );
}

#[test]
fn save_requires_verified_complete_payload_and_changed_native_snapshot() {
    let job = producer_job(
        &candidates(),
        &ToolCatalog::pinned(),
        "ubuntu-26.04",
        &mise(),
        env!("CARGO_PKG_VERSION"),
        &selection(),
    )
    .expect("job");
    let save = job
        .steps
        .iter()
        .find(|step| {
            step.id
                .as_ref()
                .is_some_and(|id| id.as_str() == "velnor-bun-source-save")
        })
        .expect("save");
    let gate = save.condition.as_deref().expect("save condition");
    assert!(gate.contains("steps.velnor-bun-public-proof.outputs.verified == 'true'"));
    assert!(gate.contains("VELNOR_BUN_DOWNLOADS_SNAPSHOT_CHANGED == 'true'"));
}

#[test]
fn producer_rejects_foreign_runtime_and_nonexplicit_selection() {
    let catalog = ToolCatalog::pinned();
    assert!(
        producer_job(
            &candidates(),
            &catalog,
            "ubuntu-26.04",
            &mise(),
            "unqualified-runtime",
            &selection()
        )
        .is_err()
    );
    assert!(
        source_records(
            &candidates(),
            &catalog,
            "ubuntu-26.04",
            &mise(),
            "unqualified-runtime",
            &selection()
        )
        .is_err()
    );
    for unsupported in [
        ToolProducerSelection::default(),
        ToolProducerSelection {
            unconditional: true,
            ..selection()
        },
        ToolProducerSelection {
            cargo_fallback: true,
            ..selection()
        },
    ] {
        assert!(
            producer_job(
                &candidates(),
                &catalog,
                "ubuntu-26.04",
                &mise(),
                env!("CARGO_PKG_VERSION"),
                &unsupported
            )
            .is_err()
        );
    }
}
