use super::super::record;
use super::{metadata, metadata_with_role};
use velnor_actions_contract::{SourceBoundOperation, SourceProducerRole};

#[test]
fn report_factory_binds_restore_and_publication_evidence() {
    let helper = record(&metadata(), "0.1.0").expect("compiled report");
    assert_eq!(
        helper.invocation().descriptor().operation(),
        SourceBoundOperation::SourceProducerReport
    );
    let env = helper.environment();
    for (name, value) in [
        ("VELNOR_SOURCE_IDENTITY", "source-key"),
        ("VELNOR_SOURCE_OUTCOME", "${{ steps.verify.outcome }}"),
        (
            "VELNOR_SOURCE_VERIFIED",
            "${{ steps.verify.outputs.verified }}",
        ),
        ("VELNOR_SOURCE_ERROR", "${{ steps.verify.outputs.error }}"),
        ("VELNOR_SOURCE_SAVE_OUTCOME", "${{ steps.save.outcome }}"),
        (
            "VELNOR_SOURCE_RESTORE_OUTCOME",
            "${{ steps.restore.outcome }}",
        ),
        (
            "VELNOR_SOURCE_RESTORE_KEY",
            "${{ steps.restore.outputs.cache-matched-key }}",
        ),
        (
            "VELNOR_SOURCE_PUBLICATION_OUTCOME",
            "${{ steps.publication.outcome }}",
        ),
        (
            "VELNOR_SOURCE_PUBLICATION_MATCHED_KEY",
            "${{ steps.publication.outputs.cache-matched-key }}",
        ),
        (
            "VELNOR_SOURCE_PUBLICATION_EXPECTED_KEY",
            "source-key-snapshot-${{env.VELNOR_NPM_DOWNLOADS_SNAPSHOT_DIGEST}}-${{github.run_id}}-${{github.run_attempt}}",
        ),
        (
            "VELNOR_SOURCE_SNAPSHOT_OUTCOME",
            "${{ steps.velnor-npm-source-after.outcome }}",
        ),
        (
            "VELNOR_SOURCE_SNAPSHOT_CHANGED",
            "${{ env.VELNOR_NPM_DOWNLOADS_SNAPSHOT_CHANGED }}",
        ),
    ] {
        assert_eq!(env[name], value);
    }
}

#[test]
fn report_factory_binds_each_role_snapshot_evidence() {
    for (role, outcome, changed, expected_key) in [
        (
            SourceProducerRole::Bun,
            "${{ steps.velnor-bun-source-after.outcome }}",
            "${{ env.VELNOR_BUN_DOWNLOADS_SNAPSHOT_CHANGED }}",
            "source-key-snapshot-${{env.VELNOR_BUN_DOWNLOADS_SNAPSHOT_DIGEST}}-${{github.run_id}}-${{github.run_attempt}}",
        ),
        (
            SourceProducerRole::Cargo,
            "${{ steps.velnor-rust-source-after.outcome }}",
            "${{ env.VELNOR_SOURCES_SNAPSHOT_CHANGED }}",
            "source-key-snapshot-${{env.VELNOR_SOURCES_SNAPSHOT_DIGEST}}-${{github.run_id}}-${{github.run_attempt}}",
        ),
        (SourceProducerRole::Tofu, "success", "true", "source-key"),
    ] {
        let helper = record(&metadata_with_role(role), "0.1.0").expect("compiled report");
        let env = helper.environment();
        assert_eq!(env["VELNOR_SOURCE_SNAPSHOT_OUTCOME"], outcome);
        assert_eq!(env["VELNOR_SOURCE_SNAPSHOT_CHANGED"], changed);
        assert_eq!(env["VELNOR_SOURCE_PUBLICATION_EXPECTED_KEY"], expected_key);
    }
}
