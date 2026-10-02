//! Tofu identity-extension cases (T12).
use velnor_actions_contract::{
    cachekey::TOFU_EXTENSION_SCHEMA, digest_b3, validate_tofu_extension,
};
use velnor_actions_tofu::identity::{
    TofuGroupExtensionInputs, entry_metadata_for_task, extension_for_proposal, lock_slot_at_root,
};
use velnor_actions_tofu::kinds::TofuTaskKind;
use velnor_actions_tofu::propose::{TofuTaskGroup, propose_task};
use velnor_actions_tofu::task_identity::{DigestSlot, ExtensionInputs, TofuTaskIdentityExtension};

use crate::support::TempDir;

/// Extension inputs over fixed digests for one root/kind.
fn inputs<'a>(
    unit_id: &'a str,
    root: &'a str,
    lock: DigestSlot,
    kind: TofuTaskKind,
    workspace: &'a str,
    graph: &'a str,
    config: &'a str,
) -> ExtensionInputs<'a> {
    ExtensionInputs {
        unit_id,
        workspace_id: workspace,
        profile: "default",
        manifest: if root.is_empty() { "." } else { root },
        graph_digest: graph,
        root,
        config_digest: config,
        lock_digest: lock,
        kind,
        undeclared_reads: false,
        declared_inputs: &[],
    }
}

/// The derived extension carries the tofu schema and validates.
#[test]
fn extension_carries_schema_and_validates() {
    let workspace = digest_b3(b"workspace");
    let graph = digest_b3(b"graph");
    let config = digest_b3(b"config");
    let ext = TofuTaskIdentityExtension::for_task(&inputs(
        "root",
        "",
        DigestSlot::AbsentProven("not_found:.terraform.lock.hcl".to_owned()),
        TofuTaskKind::Validate,
        &workspace,
        &graph,
        &config,
    ));
    assert_eq!(ext.unit_id, "root");
    assert_eq!(ext.kind, "validate");
    assert_eq!(ext.driver, "tofu+none");
    assert!(ext.lock_digest.is_none());
    assert!(ext.reuse_eligible().is_ok());
    let envelope = ext.to_stack_extension();
    assert_eq!(envelope.schema, TOFU_EXTENSION_SCHEMA);
    assert_eq!(validate_tofu_extension(&envelope), Ok(()));
}

/// A known lockfile binds content; an unknown one blocks reuse.
#[test]
fn lock_slot_gates_reuse() {
    let workspace = digest_b3(b"workspace");
    let graph = digest_b3(b"graph");
    let config = digest_b3(b"config");
    let known = TofuTaskIdentityExtension::for_task(&inputs(
        "stacks/a",
        "stacks/a",
        DigestSlot::Known(digest_b3(b"lock")),
        TofuTaskKind::InitForValidate,
        &workspace,
        &graph,
        &config,
    ));
    assert!(known.lock_digest.is_some());
    assert!(known.reuse_eligible().is_ok());
    assert_eq!(validate_tofu_extension(&known.to_stack_extension()), Ok(()));
    let unknown = TofuTaskIdentityExtension::for_task(&inputs(
        "stacks/a",
        "stacks/a",
        DigestSlot::Unknown("unreadable".to_owned()),
        TofuTaskKind::InitForValidate,
        &workspace,
        &graph,
        &config,
    ));
    let err = unknown.reuse_eligible().expect_err("unknown blocks reuse");
    assert!(
        err.to_string().contains("unresolved_input:lockfile"),
        "{err}"
    );
}

/// Undeclared reads block reuse even with a known lockfile.
#[test]
fn undeclared_reads_block_reuse() {
    let workspace = digest_b3(b"workspace");
    let graph = digest_b3(b"graph");
    let config = digest_b3(b"config");
    let mut with_reads = inputs(
        "root",
        "",
        DigestSlot::Known(digest_b3(b"lock")),
        TofuTaskKind::Fmt,
        &workspace,
        &graph,
        &config,
    );
    with_reads.undeclared_reads = true;
    let ext = TofuTaskIdentityExtension::for_task(&with_reads);
    let err = ext.reuse_eligible().expect_err("reads block reuse");
    assert!(err.to_string().contains("undeclared_inputs"), "{err}");
}

/// Slot states keep the Unknown-vs-absence distinction.
#[test]
fn slot_states_stay_distinct() {
    assert!(DigestSlot::Known(digest_b3(b"x")).as_known().is_some());
    assert!(
        DigestSlot::AbsentProven("e".to_owned())
            .as_known()
            .is_none()
    );
    assert!(!DigestSlot::AbsentProven("e".to_owned()).is_unknown());
    assert!(DigestSlot::Unknown("r".to_owned()).is_unknown());
    assert_ne!(
        DigestSlot::AbsentProven("e".to_owned()).state(),
        DigestSlot::Unknown("r".to_owned()).state()
    );
}

/// The proposal bridge derives the extension; drift fails closed.
#[test]
fn proposal_bridge_derives_and_rejects_drift() {
    let group = TofuTaskGroup {
        root: String::new(),
        kind: TofuTaskKind::Validate,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = propose_task(&group).expect("proposes");
    let workspace = digest_b3(b"workspace");
    let graph = digest_b3(b"graph");
    let config = digest_b3(b"config");
    let bridge = TofuGroupExtensionInputs {
        unit_id: "root",
        workspace_id: &workspace,
        profile: "default",
        manifest: ".",
        graph_digest: &graph,
        root: "",
        config_digest: &config,
        lock_digest: DigestSlot::AbsentProven("not_found:.terraform.lock.hcl".to_owned()),
    };
    let ext = extension_for_proposal(&task, &bridge).expect("bridge derives");
    assert_eq!(ext.unit_id, "root");
    assert_eq!(ext.task_kind, TofuTaskKind::Validate);
    assert_eq!(validate_tofu_extension(&ext.to_stack_extension()), Ok(()));
    let mut drifted = task.clone();
    drifted.task_kind = "plan".to_owned();
    assert!(extension_for_proposal(&drifted, &bridge).is_err());
    let mut drifted = task.clone();
    drifted.identity.compile_driver = "cargo".to_owned();
    let err = extension_for_proposal(&drifted, &bridge).expect_err("driver drift");
    assert!(err.to_string().contains("unknown_driver:cargo"), "{err}");
    let mut drifted = task;
    drifted.identity.test_runner = "cargo_test".to_owned();
    let err = extension_for_proposal(&drifted, &bridge).expect_err("runner drift");
    assert!(
        err.to_string().contains("unknown_runner:cargo_test"),
        "{err}"
    );
}

/// Entry metadata carries tofu spellings; drift fails closed.
#[test]
fn entry_metadata_pins_tofu_shape() {
    let group = TofuTaskGroup {
        root: "stacks/a".to_owned(),
        kind: TofuTaskKind::InitForValidate,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = propose_task(&group).expect("proposes");
    let meta = entry_metadata_for_task(&task, &[]).expect("metadata");
    assert_eq!(meta["unit_id"], serde_json::json!("stacks/a"));
    assert_eq!(meta["manifest_key"], serde_json::json!("stacks/a"));
    assert_eq!(meta["kind"], serde_json::json!("init"));
    assert_eq!(meta["compile_driver"], serde_json::json!("tofu"));
    assert_eq!(meta["test_runner"], serde_json::json!("none"));
    let mut drifted = task;
    drifted.identity.compile_driver = "mbx".to_owned();
    assert!(entry_metadata_for_task(&drifted, &[]).is_err());
}

/// The lockfile probe binds content, absence, or explicit unknown.
#[test]
fn lock_probe_binds_content_or_absence() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::create("tofu-lock")?;
    let absent = lock_slot_at_root(dir.path(), ".");
    assert!(matches!(absent, DigestSlot::AbsentProven(_)));
    dir.write(".terraform.lock.hcl", "lock content")?;
    let known = lock_slot_at_root(dir.path(), ".");
    assert_eq!(known, DigestSlot::Known(digest_b3(b"lock content")));
    dir.write("stacks/a/main.tf", "terraform {}")?;
    let nested_absent = lock_slot_at_root(dir.path(), "stacks/a");
    assert!(matches!(nested_absent, DigestSlot::AbsentProven(_)));
    Ok(())
}
