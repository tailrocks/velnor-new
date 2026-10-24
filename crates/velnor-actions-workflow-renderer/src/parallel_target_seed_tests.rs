use super::*;
use velnor_actions_contract::StepKind;

#[test]
fn native_seed_request_is_sequential_and_uses_only_closed_environment() {
    let step = seed_parallel_targets_step("crates/work/target", &["velnor-clippy", "velnor-doc"])
        .expect("fixed native preparation");
    assert!(!step.parallel);
    assert_eq!(step.role, None);
    let StepKind::Internal { operation, env } = step.kind else {
        panic!("internal staged binary operation")
    };
    assert_eq!(operation, "seed-parallel-targets-v1");
    assert_eq!(
        env,
        BTreeMap::from([
            (
                "VELNOR_TARGET_ROOT".to_owned(),
                "crates/work/target".to_owned()
            ),
            (
                "VELNOR_TARGET_LANES".to_owned(),
                "velnor-clippy,velnor-doc".to_owned()
            ),
        ])
    );
}

#[test]
fn native_seed_request_rejects_paths_lanes_and_shell_injection() {
    for target in [
        "/target",
        "../target",
        "a//target",
        "a/../target",
        "target/",
        "a;id/target",
        "other",
    ] {
        assert!(
            seed_parallel_targets_step(target, &["velnor-clippy"]).is_err(),
            "{target}"
        );
    }
    for lanes in [
        vec![],
        vec!["velnor-clippy", "velnor-clippy"],
        vec!["velnor-doc", "velnor-clippy"],
        vec!["../doc"],
        vec!["velnor-doc;id"],
    ] {
        assert!(seed_parallel_targets_step("target", &lanes).is_err());
    }
}
