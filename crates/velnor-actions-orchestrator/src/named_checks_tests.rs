//! Opaque checks bind source, configuration and their own target identity.
use super::*;
use tempfile::TempDir;
use velnor_actions_contract::{
    CheckExecutor, CheckPlatform, CheckRunner, MiseCheck, PlanGenerator,
};
fn fixture() -> (TempDir, MiseCheck) {
    let dir = TempDir::new().expect("temporary repository");
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tasks.verify]\nrun = 'echo check'\n",
    )
    .expect("native task");
    std::fs::write(dir.path().join("input.txt"), "first").expect("input");
    let check = MiseCheck {
        id: "verify".to_owned(),
        task: "verify".to_owned(),
        directory: ".".to_owned(),
        runner: CheckRunner {
            label: "ubuntu-24.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec!["input.txt".to_owned()],
        tools: Vec::new(),
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 10,
    };
    (dir, check)
}
fn discovered_check(root: &Path, check: &MiseCheck) -> DiscoveredCheck {
    velnor_actions_mise::discover_checks(root, std::slice::from_ref(check), &[])
        .expect("static checks")
        .remove(0)
}
fn generator() -> PlanGenerator {
    PlanGenerator {
        version: "0.1.0".to_owned(),
        target: "x86_64-unknown-linux-gnu".to_owned(),
        sha256: "a".repeat(64),
    }
}
#[test]
fn named_check_without_cargo_has_one_executing_obligation() {
    let (dir, check) = fixture();
    assert!(!dir.path().join("Cargo.toml").exists());
    let item = discovered_check(dir.path(), &check);
    let (obligation, entry) = plan::derive(
        dir.path(),
        &item,
        "local",
        &generator(),
        &ToolCatalog::pinned(),
    )
    .expect("plan check");
    assert_eq!(
        obligation.decision,
        velnor_actions_contract::ObligationDecision::Execute
    );
    assert_eq!(obligation.reason, "opaque_check_requires_execution");
    assert_eq!(entry.job_id, "check-verify");
    assert!(entry.cache_ids.is_none());
    assert_eq!(entry.adapter_metadata["task_cache_enabled"], false);
    assert_eq!(entry.adapter_metadata["artifact_cache_enabled"], false);
}
#[test]
fn declared_source_and_native_task_edits_change_identity() {
    let (dir, check) = fixture();
    let derive = || {
        let item = discovered_check(dir.path(), &check);
        plan::derive(
            dir.path(),
            &item,
            "local",
            &generator(),
            &ToolCatalog::pinned(),
        )
        .expect("plan")
        .0
    };
    let before = derive();
    std::fs::write(dir.path().join("input.txt"), "second").expect("edit declared source");
    let source_edit = derive();
    assert_ne!(before.input_digest, source_edit.input_digest);
    assert_ne!(before.closure_digest, source_edit.closure_digest);
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tasks.verify]\nrun = 'echo changed'\n",
    )
    .expect("edit native task");
    assert_ne!(source_edit.input_digest, derive().input_digest);
}

#[test]
fn binary_declared_input_changes_digest_and_stays_executable() {
    let (dir, check) = fixture();
    std::fs::write(dir.path().join("image.png"), [0x89, b'P', b'N', b'G', 0xff])
        .expect("binary input");
    let mut binary_check = check;
    binary_check.inputs = vec!["image.png".to_owned()];
    let derive = || {
        let item = discovered_check(dir.path(), &binary_check);
        plan::derive(
            dir.path(),
            &item,
            "local",
            &generator(),
            &ToolCatalog::pinned(),
        )
        .expect("binary input plans")
    };
    let (first, _) = derive();
    assert_eq!(
        first.decision,
        velnor_actions_contract::ObligationDecision::Execute
    );
    std::fs::write(dir.path().join("image.png"), [0x89, b'P', b'N', b'G', 0xfe])
        .expect("mutate binary input");
    let (second, _) = derive();
    assert_eq!(
        second.decision,
        velnor_actions_contract::ObligationDecision::Execute
    );
    assert_ne!(first.input_digest, second.input_digest);
    assert_ne!(first.closure_digest, second.closure_digest);
}
#[test]
fn check_platform_ignores_global_linux_label() {
    let (dir, check) = fixture();
    let linux = discovered_check(dir.path(), &check);
    let mut mac = check;
    mac.runner.label = "orbstack-test".to_owned();
    mac.runner.platform = CheckPlatform::MacosArm64;
    mac.runner.executor = CheckExecutor::EphemeralSelfHosted;
    let mac = discovered_check(dir.path(), &mac);
    let first =
        crate::internal_plan::identities::platform_id_for_group("ubuntu-24.04", &linux.proposal)
            .expect("linux identity");
    let second =
        crate::internal_plan::identities::platform_id_for_group("ubuntu-24.04", &mac.proposal)
            .expect("mac identity");
    assert_ne!(first, second);
    assert_eq!(
        second,
        crate::internal_plan::identities::platform_id_for_group("unrelated", &mac.proposal)
            .expect("independent label")
    );
}
#[test]
fn opaque_closure_and_wrong_stack_never_qualify() {
    let (dir, check) = fixture();
    let mut item = discovered_check(dir.path(), &check);
    let digest = digest_b3(b"bound");
    let closure = resolve_closure(dir.path(), &item.proposal, &digest, &digest, &digest)
        .expect("source closure");
    assert!(closure.verify_complete().is_err());
    assert!(closure.unknown_inputs().contains(&"opaque_task_state"));
    item.proposal.stack_id = "rust".to_owned();
    assert!(metadata_for(&item).is_err());
    assert!(extension_for(&item).is_err());
    assert!(resolve_closure(dir.path(), &item.proposal, &digest, &digest, &digest).is_err());
}

#[test]
fn named_check_hosted_label_and_target_must_agree() {
    let (dir, check) = fixture();
    let mut item = discovered_check(dir.path(), &check);
    item.proposal.runner_profile = "macos-15".to_owned();
    assert!(
        crate::internal_plan::identities::platform_id_for_group("ubuntu-24.04", &item.proposal)
            .is_err()
    );
}
