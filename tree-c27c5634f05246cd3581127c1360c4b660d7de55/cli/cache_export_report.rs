use super::*;

pub(super) struct ExportReportInput<'a> {
    pub(super) outcome: &'a store::TransferOutcome,
    pub(super) snapshot_budget: Option<u64>,
    pub(super) delta: Option<&'a super::super::cache_comparison::Delta>,
    pub(super) semantic_digest: &'a str,
    pub(super) capture: &'a str,
    pub(super) capture_reason: Option<&'a str>,
    pub(super) usefulness_reason: WorkspaceUsefulnessReason,
}

pub(super) fn export_report(input: ExportReportInput<'_>) -> serde_json::Value {
    serde_json::json!({
        "version": 2, "budget_refused": false, "snapshot_budget_bytes": input.snapshot_budget, "exported": input.outcome.exported, "actions": input.outcome.actions, "objects": input.outcome.objects,
        "bytes": input.outcome.bytes,
        "emitted_bundle_useful_delta": input.delta.map_or(input.outcome.actions > 0, |d| d.useful()),
        "workspace_usefulness": WorkspaceUsefulness {status: WorkspaceUsefulnessStatus::Unavailable, reason: input.usefulness_reason},
        "delta": input.delta,
        "semantic_digest": input.semantic_digest,
        "workspace_comparison": "relative_path_type_content_mode_symlink_target",
        "workspace_comparison_exclusions": ["effective_build_root/.rustc_info.json"],
        "workspace_transport_scope": "recorded_target_and_build_directories",
        "workspace_capture": input.capture,
        "workspace_capture_unavailable_reason": input.capture_reason,
        "workspace_persistence_verified": false,
        "qualification": if input.capture == "captured" { "compiled actions, predictions and Cargo unit state; includes Cargo output and intermediate roots; excludes only effective build-root compiler-query cache .rustc_info.json; other scheduler content differences are reported, not proven additional cache hits" } else { "compiled actions and predictions; Cargo workspace capture unavailable; see workspace_capture_unavailable_reason; workspace persistence not verified" }
    })
}
