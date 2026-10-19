//! Closed source bindings for original preview and retained-release continuation.
#[derive(Clone, Copy)]
pub(super) enum TapInvocationContext {
    Preview,
    Reconcile,
}
impl TapInvocationContext {
    pub(super) const fn selection(self) -> &'static str {
        match self {
            Self::Preview => "preview-eligibility",
            Self::Reconcile => "preview-tap-selection",
        }
    }
    pub(super) const fn release_inputs(self) -> [(&'static str, &'static str); 3] {
        match self {
            Self::Preview => [
                (
                    "VELNOR_RELEASE_COMMIT_COUNT",
                    "${{ needs.preview-eligibility.outputs.commit_count }}",
                ),
                (
                    "VELNOR_RELEASE_CI_RUN_ID",
                    "${{ needs.preview-eligibility.outputs.ci_run_id }}",
                ),
                (
                    "VELNOR_RELEASE_CI_RUN_ATTEMPT",
                    "${{ needs.preview-eligibility.outputs.ci_attempt }}",
                ),
            ],
            Self::Reconcile => [
                (
                    "VELNOR_RELEASE_COMMIT_COUNT",
                    "${{ needs.preview-tap-selection.outputs.commit_count }}",
                ),
                (
                    "VELNOR_RELEASE_CI_RUN_ID",
                    "${{ needs.preview-tap-selection.outputs.ci_run_id }}",
                ),
                (
                    "VELNOR_RELEASE_CI_RUN_ATTEMPT",
                    "${{ needs.preview-tap-selection.outputs.ci_attempt }}",
                ),
            ],
        }
    }
    pub(super) const fn original(self) -> [(&'static str, &'static str); 2] {
        match self {
            Self::Preview => [
                (
                    "VELNOR_RELEASE_ORIGINAL_RUN_ID",
                    "${{ inputs.original_run_id }}",
                ),
                (
                    "VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT",
                    "${{ inputs.original_run_attempt }}",
                ),
            ],
            Self::Reconcile => [
                (
                    "VELNOR_RELEASE_ORIGINAL_RUN_ID",
                    "${{ needs.preview-tap-selection.outputs.original_run_id }}",
                ),
                (
                    "VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT",
                    "${{ needs.preview-tap-selection.outputs.original_run_attempt }}",
                ),
            ],
        }
    }
    pub(super) const fn source(self) -> &'static str {
        match self {
            Self::Preview => "${{ needs.preview-eligibility.outputs.source_sha }}",
            Self::Reconcile => "${{ needs.preview-tap-selection.outputs.source_sha }}",
        }
    }
}
