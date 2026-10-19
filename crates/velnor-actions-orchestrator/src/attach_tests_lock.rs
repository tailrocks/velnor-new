//! Lock-acquisition workflow tests.

use super::*;
use crate::publish_job::baseline_publish_job;

#[test]
fn lock_acquire_inserts_digest_verified_stage() {
    use velnor_actions_contract::{GeneratorBinary, LockedGenerator, MiseBootstrap};
    let lock = GeneratorLock {
        schema: 1,
        generator: LockedGenerator {
            binary: "velnor-actions".to_owned(),
            version: "0.1.0".to_owned(),
            commit: "ab".repeat(20),
            binaries: vec![GeneratorBinary {
                target: "x86_64-unknown-linux-gnu".to_owned(),
                artifact: "https://example.invalid/r".to_owned(),
                sha256: "a".repeat(64),
            }],
        },
        mise_bootstrap: MiseBootstrap {
            version: velnor_actions_mise::MISE_VERSION.to_owned(),
            artifact: "https://example.invalid/m".to_owned(),
            sha256: "b".repeat(64),
        },
        actions: Vec::new(),
    };
    let catalog = ToolCatalog::pinned();
    let mut ir = bare_ir(BTreeMap::from([
        (
            "plan".to_owned(),
            plan_job("ubuntu-26.04", None, &catalog, rust_plan_needs(), &[], &[])
                .expect("plan job"),
        ),
        (
            "required".to_owned(),
            final_job("ubuntu-26.04", &[], None, &catalog).expect("final job"),
        ),
        (
            "publish-baseline".to_owned(),
            baseline_publish_job("ubuntu-26.04", "main", None).expect("publish job"),
        ),
    ]));
    assert!(attach_lock_acquire(&mut ir, &lock, "ubuntu-26.04", "0.1.0").is_ok());
    assert_eq!(ir.jobs["publish-baseline"].steps[0].name, "Checkout");
    assert_eq!(ir.jobs["publish-baseline"].steps[1].name, "Acquire Velnor");
    let names: Vec<&str> = ir.jobs["plan"]
        .steps
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Checkout",
            "Prepare pinned tools",
            "Acquire Velnor",
            "Prepare Rust components",
            "Write request",
            "Plan"
        ]
    );
    let names: Vec<&str> = ir.jobs["required"]
        .steps
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Acquire Velnor",
            "Prepare pinned tools",
            "Write request",
            "Merge reports"
        ]
    );
    assert!(attach_lock_acquire(&mut ir, &lock, "ubuntu-26.04-arm", "0.1.0").is_err());
}

#[test]
fn lock_acquire_records_source_commit() {
    use velnor_actions_contract::{GeneratorBinary, LockedGenerator, MiseBootstrap, StepKind};
    use velnor_actions_workflow_renderer::steps::RELEASE_COMMIT_ENV;
    let lock = GeneratorLock {
        schema: 1,
        generator: LockedGenerator {
            binary: "velnor-actions".to_owned(),
            version: "0.1.0".to_owned(),
            commit: "cd".repeat(20),
            binaries: vec![GeneratorBinary {
                target: "x86_64-unknown-linux-gnu".to_owned(),
                artifact: "https://example.invalid/r".to_owned(),
                sha256: "a".repeat(64),
            }],
        },
        mise_bootstrap: MiseBootstrap {
            version: velnor_actions_mise::MISE_VERSION.to_owned(),
            artifact: "https://example.invalid/m".to_owned(),
            sha256: "b".repeat(64),
        },
        actions: Vec::new(),
    };
    let catalog = ToolCatalog::pinned();
    let mut ir = bare_ir(BTreeMap::from([(
        "plan".to_owned(),
        plan_job("ubuntu-26.04", None, &catalog, rust_plan_needs(), &[], &[]).expect("plan job"),
    )]));
    ir.jobs.insert(
        "required".to_owned(),
        final_job("ubuntu-26.04", &[], None, &catalog).expect("final job"),
    );
    attach_lock_acquire(&mut ir, &lock, "ubuntu-26.04", "0.1.0").expect("attach");
    for id in ["plan", "required"] {
        let acquire = ir.jobs[id]
            .steps
            .iter()
            .find(|step| step.name == "Acquire Velnor")
            .expect("acquire step");
        let StepKind::Shell { env, .. } = &acquire.kind else {
            panic!("acquire must be a shell step");
        };
        assert_eq!(
            env.get(RELEASE_COMMIT_ENV).map(String::as_str),
            Some("cd".repeat(20).as_str()),
            "lock path must record the commit like the manifest path"
        );
    }
}
