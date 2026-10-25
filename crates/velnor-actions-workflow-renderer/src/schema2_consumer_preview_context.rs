#[derive(Clone, Copy)]
pub(in crate::schema2) enum TapInvocationContext {
    Preview,
    Stable,
    StableAuthority,
    Reconcile,
    StableReconcile,
}
impl TapInvocationContext {
    pub(in crate::schema2) const fn for_channel(
        channel: velnor_actions_contract::package_release::PackageReleaseChannel,
    ) -> Self {
        match channel {
            velnor_actions_contract::package_release::PackageReleaseChannel::Preview => {
                Self::Preview
            }
            velnor_actions_contract::package_release::PackageReleaseChannel::Stable => Self::Stable,
        }
    }
    pub(in crate::schema2) const fn prefix(self) -> &'static str {
        match self {
            Self::Preview | Self::Reconcile => "preview",
            Self::Stable | Self::StableAuthority | Self::StableReconcile => "stable",
        }
    }
    pub(in crate::schema2) const fn production_if(self) -> &'static str {
        match self {
            Self::Preview | Self::Reconcile => "${{ github.event_name == 'workflow_run' }}",
            Self::Stable | Self::StableAuthority | Self::StableReconcile => {
                "${{ github.event_name == 'workflow_dispatch' && inputs.operation == 'release' && inputs.original_run_id == '' && inputs.original_run_attempt == '' }}"
            }
        }
    }
    pub(in crate::schema2) const fn environment(self) -> &'static str {
        match self {
            Self::Preview | Self::Reconcile => "preview-publish",
            Self::Stable | Self::StableAuthority | Self::StableReconcile => "release",
        }
    }
    pub(in crate::schema2) const fn selection(self) -> &'static str {
        match self {
            Self::Preview => "preview-eligibility",
            Self::Stable => "stable-tag",
            Self::StableAuthority => "stable-eligibility",
            Self::Reconcile => "preview-tap-selection",
            Self::StableReconcile => "stable-tap-selection",
        }
    }
    pub(in crate::schema2) const fn release_inputs(self) -> [(&'static str, &'static str); 3] {
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
            Self::Stable => [
                (
                    "VELNOR_RELEASE_COMMIT_COUNT",
                    "${{ needs.stable-tag.outputs.commit_count }}",
                ),
                (
                    "VELNOR_RELEASE_CI_RUN_ID",
                    "${{ needs.stable-tag.outputs.ci_run_id }}",
                ),
                (
                    "VELNOR_RELEASE_CI_RUN_ATTEMPT",
                    "${{ needs.stable-tag.outputs.ci_attempt }}",
                ),
            ],
            Self::StableAuthority => [
                (
                    "VELNOR_RELEASE_COMMIT_COUNT",
                    "${{ needs.stable-eligibility.outputs.commit_count }}",
                ),
                (
                    "VELNOR_RELEASE_CI_RUN_ID",
                    "${{ needs.stable-eligibility.outputs.ci_run_id }}",
                ),
                (
                    "VELNOR_RELEASE_CI_RUN_ATTEMPT",
                    "${{ needs.stable-eligibility.outputs.ci_attempt }}",
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
            Self::StableReconcile => [
                (
                    "VELNOR_RELEASE_COMMIT_COUNT",
                    "${{ needs.stable-tap-selection.outputs.commit_count }}",
                ),
                (
                    "VELNOR_RELEASE_CI_RUN_ID",
                    "${{ needs.stable-tap-selection.outputs.ci_run_id }}",
                ),
                (
                    "VELNOR_RELEASE_CI_RUN_ATTEMPT",
                    "${{ needs.stable-tap-selection.outputs.ci_attempt }}",
                ),
            ],
        }
    }
    pub(in crate::schema2) const fn original(self) -> [(&'static str, &'static str); 2] {
        match self {
            Self::Preview | Self::Stable | Self::StableAuthority => [
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
            Self::StableReconcile => [
                (
                    "VELNOR_RELEASE_ORIGINAL_RUN_ID",
                    "${{ needs.stable-tap-selection.outputs.original_run_id }}",
                ),
                (
                    "VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT",
                    "${{ needs.stable-tap-selection.outputs.original_run_attempt }}",
                ),
            ],
        }
    }
    pub(in crate::schema2) const fn source(self) -> &'static str {
        match self {
            Self::Preview => "${{ needs.preview-eligibility.outputs.source_sha }}",
            Self::Stable => "${{ needs.stable-tag.outputs.source_sha }}",
            Self::StableAuthority => "${{ needs.stable-eligibility.outputs.source_sha }}",
            Self::Reconcile => "${{ needs.preview-tap-selection.outputs.source_sha }}",
            Self::StableReconcile => "${{ needs.stable-tap-selection.outputs.source_sha }}",
        }
    }
}
