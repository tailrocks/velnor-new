//! Source-only fixtures describe IR; none construct action/tool/origin authority.
use super::*;
use velnor_actions_contract::{MbxCacheDomain, MbxOwnerIdentity};

fn descriptor() -> MbxExportDescriptor {
    MbxExportDescriptor {
        domain: MbxCacheDomain::Validation,
        producer_job_id: "rust-owner".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        target: "x86_64-unknown-linux-gnu".to_owned(),
        workspace_roots: vec![".".to_owned()],
        configuration_digest: velnor_actions_contract::canonical::digest_b3(b"context-fixture"),
        task_digests: BTreeMap::from([(
            "stack/rust/root/clippy/default".to_owned(),
            velnor_actions_contract::canonical::digest_b3(b"task-fixture"),
        )]),
        owner: MbxOwnerIdentity {
            version: "1.21.1-owned-cache-transport".to_owned(),
            binary_sha256: "a".repeat(64),
            qualification_identity: "b".repeat(64),
            source_sha: "c".repeat(40),
        },
        action_sha: "d".repeat(40),
    }
}

#[test]
fn descriptive_fixture_lowers_only_strict_preinstalled_source_draft() {
    let descriptor = descriptor();
    let step = lower_restore(
        &descriptor,
        &ToolCatalog::pinned(),
        RestoreSource {
            uses: format!("jdx/mr-boxington-action@{}", descriptor.action_sha),
            path: "installs/github-jdx-mr-boxington/owned/bin/mbx",
            version: &descriptor.owner.version,
            binary_sha256: &descriptor.owner.binary_sha256,
        },
    )
    .expect("detached source lowering");
    assert_eq!(step.id.as_ref().map(StepId::as_str), Some("mbx-restore"));
    assert!(step.name.contains("not activated"));
    let StepKind::Action { with, .. } = step.kind else {
        panic!("source draft must describe an action");
    };
    assert!(
        !with.contains_key("version"),
        "no released installer fallback"
    );
    assert_eq!(with["expected-version"], descriptor.owner.version);
    assert_eq!(
        with["expected-binary-sha256"],
        descriptor.owner.binary_sha256
    );
    assert_eq!(
        with["comparison-state"],
        descriptor.comparison_path().expect("comparison")
    );
    assert_eq!(
        with["export-group"],
        descriptor.export_group().expect("group")
    );
    assert!(with["restore-keys"].ends_with("snapshot-"));
    assert!(QualifiedMbxAction::require_comparison_export().is_err());
}

#[test]
fn late_upload_requires_actual_successful_useful_export() {
    let step = crate::mbx_export::upload_step(&descriptor()).expect("source upload draft");
    assert_eq!(
        step.condition.as_deref(),
        Some(
            "success() && steps.mbx-export.outcome == 'success' && steps.mbx-export.outputs.emitted_bundle_useful_delta == 'true'"
        )
    );
    let StepKind::Action { with, .. } = step.kind else {
        panic!("source upload action");
    };
    assert_eq!(
        with["name"],
        descriptor().artifact_name().expect("artifact")
    );
    assert_eq!(with["path"], descriptor().bundle_root().expect("bundle"));
}
