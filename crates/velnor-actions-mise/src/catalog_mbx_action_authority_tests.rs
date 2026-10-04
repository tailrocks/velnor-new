//! Fixture-only identity mutations never publish action authority.

use super::*;

#[test]
fn old_official_pin_and_tool_authority_do_not_grant_action_behavior() {
    assert!(QualifiedMbxAction::require_comparison_export().is_err());
}

#[test]
fn every_action_field_binds_its_own_qualification_identity() {
    let fixture = QualifiedMbxAction {
        source_repository: "https://fixture.invalid/action",
        source_commit: "fixture-commit",
        source_tree: "fixture-tree",
        release_version: "0.0.1-fixture",
        dist_sha256: "fixture-dist",
        behavior_abi: "fixture-behavior",
        output_abi: "fixture-output",
    };
    let mutations: [fn(&mut QualifiedMbxAction); 7] = [
        |value| value.source_repository = "changed-repository",
        |value| value.source_commit = "changed-commit",
        |value| value.source_tree = "changed-tree",
        |value| value.release_version = "changed-version",
        |value| value.dist_sha256 = "changed-dist",
        |value| value.behavior_abi = "changed-behavior",
        |value| value.output_abi = "changed-output",
    ];
    for mutate in mutations {
        let mut changed = fixture;
        mutate(&mut changed);
        assert_ne!(
            changed.qualification_digest(),
            fixture.qualification_digest()
        );
    }
}
