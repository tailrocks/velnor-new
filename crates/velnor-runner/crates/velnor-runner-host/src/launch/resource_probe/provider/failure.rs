#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::launch::resource_probe) enum ProviderFailure {
    TrustedSourcePinMissing,
    TrustedSourcePinInvalid,
    CredentialMissing,
    WorkspaceCreate,
    WorkspaceCleanup,
    NativeAdmission,
    Manifest,
    Archive,
    ImageLoad,
    ImageImportJournal,
    ImageInspect,
    ImageIdentity,
    ImageConfig,
    DaemonIdentity,
    Timeout,
}

impl ProviderFailure {
    pub(in crate::launch::resource_probe) const fn code(self) -> &'static str {
        match self {
            Self::TrustedSourcePinMissing => "trusted_source_pin_missing",
            Self::TrustedSourcePinInvalid => "trusted_source_pin_invalid",
            Self::CredentialMissing => "credential_missing",
            Self::WorkspaceCreate => "release_workspace_create_failed",
            Self::WorkspaceCleanup => "release_workspace_cleanup_unverified",
            Self::NativeAdmission => "native_release_admission_failed",
            Self::Manifest => "resource_probe_manifest_mismatch",
            Self::Archive => "resource_probe_archive_mismatch",
            Self::ImageLoad => "selected_daemon_image_load_failed",
            Self::ImageImportJournal => "selected_daemon_image_import_journal_failed",
            Self::ImageInspect => "selected_daemon_image_inspect_failed",
            Self::ImageIdentity => "selected_daemon_image_identity_mismatch",
            Self::ImageConfig => "selected_daemon_image_config_mismatch",
            Self::DaemonIdentity => "selected_daemon_identity_unavailable",
            Self::Timeout => "provider_timeout",
        }
    }
}
