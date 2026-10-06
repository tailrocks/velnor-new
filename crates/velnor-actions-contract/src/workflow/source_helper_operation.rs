//! Closed operation inventory and owned source path policy.
use serde::{Deserialize, Serialize};

/// Closed generated operations; adding one requires explicit source qualification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceBoundOperation {
    /// Guarded ordinary Rust compiler operation with a compiled report frame.
    RustReportWrapper,
    /// Root Linux compiler preparation.
    RustPrepareRootLinux,
    /// Desktop macOS compiler preparation.
    RustPrepareDesktopMac,
    /// Desktop source preparation without a build cache dependency.
    RustPrepareDesktopSourceMac,
    /// Root release compiler preparation for macOS without a build cache dependency.
    RustPrepareReleaseMac,
    /// Anonymous npm package proof, native ingestion and sanitization.
    NpmPublicSourceProducer,
    /// Frozen Bun cache ingestion from public proof descriptors.
    BunSourceProducer,
    /// Immutable native `OpenTofu` provider export.
    TofuProviderExport,
    /// Own the fixed root of an isolated `OpenTofu` source producer.
    TofuRootOwnership,
    /// Native export from the exact admitted MBX owner.
    MbxExport,
    /// Fresh qualified executable for an isolated MBX writer.
    MbxProducerPrepare,
    /// Authenticated same-run MBX export artifact admission.
    MbxArtifactAdmission,
    /// Data-only verification of the complete native MBX bundle.
    MbxBundleVerify,
    /// Terminal availability evidence for an isolated MBX writer.
    MbxProducerReport,
    /// Desktop native dependency hydration under the checked-out source authority.
    DesktopNativeHydration,
    /// Terminal availability evidence for an isolated native source producer.
    SourceProducerReport,
    /// Gradle dependency source production without repository task execution.
    GradleSourceProducer,
    /// Homebrew preparation owned by the compiled workload adapter.
    HomebrewPreparation,
    /// Fixed package updater fixture qualification.
    PackageUpdateFixture,
    /// Isolated owner-qualified Mise tool preparation.
    MiseToolPrepare,
    /// Terminal publication evidence for an isolated executable tool producer.
    ToolProducerReport,
    /// Digest-verified isolated Mise acquisition before any managed tool execution.
    MiseBootstrap,
    /// Materialize the catalog-owned installed Java home for a closed tool domain.
    JavaHomeMaterialize,
    /// Immutable native Pages source/artifact admission before the deployment action.
    NativePagesAdmission,
    /// Exact managed tools needed for native Pages admission.
    NativePagesToolPreparation,
    /// Public locked Rust source production without repository task execution.
    RustSourceProducer,
    /// Cached `OpenTofu` initialization from owner-qualified native sources.
    TofuCachedInit,
    /// Complete protected tag and cross-workflow full CI admission.
    NativePublishAdmission,
    /// Immutable artifact transport and exact attestation subject verification.
    NativePublishReceiptVerifier,
    /// Exact managed tools needed by closed native attestation helpers.
    NativePublishToolPreparation,
    /// Immutable Swift operation from the compiled native owner.
    NativeSwiftExecution,
    /// Immutable Rust operation from the compiled native owner.
    NativeRustExecution,
    /// OCI delivery with the complete compiled source closure.
    OciDelivery,
    /// Compiled full-CI admission for the protected default branch.
    ReleaseAdmissionDefaultBranch,
    /// Immutable protected issue observer with fixed embedded source.
    VerificationObserver,
    /// Immutable source snapshot from the read-only GitHub source API.
    RustReleaseSourceSnapshot,
    /// Fixed no-code-execution preparation of exact source-bound package bytes.
    RustReleasePreparedPackage,
    /// Anonymous verification of exact prepared package bytes in a fresh job.
    RustReleasePackageVerify,
    /// Composite receipt-owned preparation reconstructed by its compiled source owner.
    ReceiptOwnedPreparation,
    /// Anonymous immutable release packaging before any forge credential exists.
    RustReleaseAnonymousPackage,
    /// Anonymous release-plz update and immutable preparation artifact creation.
    RustReleasePrepareAnonymous,
    /// Qualified anonymous tools dedicated to immutable release preparation.
    RustReleasePrepareTools,
    /// Fixed GitHub release PR coordination from the admitted preparation artifact.
    RustReleasePrepareForge,
    /// Credential-isolated forge proof of the immutable anonymous package artifact.
    RustReleaseForgePreflight,
    /// Read-only immutable artifact transport before registry credentials exist.
    RustRegistryArtifactProof,
    /// Fixed crates.io API publication of the admitted anonymous package artifact.
    RustRegistryPublish,
    /// Fixed GitHub API publication of the admitted anonymous package artifact.
    RustForgePublish,
    /// Immutable Rust release evidence reconciliation.
    RustReleaseReconcile,
    /// Canonical observation of a closed owned cache domain.
    CacheSnapshot,
    /// Canonical manifest and authenticated producer statement input.
    CacheReceiptManifest,
    /// Verify authenticated provenance before any cached executable is consumed.
    CacheReceiptVerify,
    /// Store the exact public attestation bundle in the owned receipt directory.
    CacheReceiptBundle,
    /// Read-only immutable APT candidate verification.
    AptVerify,
    /// Protected package-feed signing and immutable stage upload preparation.
    AptStage,
    /// Read-only immutable incoming APT artifact transport.
    AptTransportIncoming,
    /// Fixed required APT delivery outcome evaluation.
    AptResult,
}

impl SourceBoundOperation {
    /// Whether this fixed operation acquires the managed-tool executable itself.
    #[must_use]
    pub const fn is_bootstrap(self) -> bool {
        matches!(self, Self::MiseBootstrap)
    }
    /// Whether this operation belongs to isolated executable tool production.
    #[must_use]
    pub const fn is_tool_producer(self) -> bool {
        matches!(self, Self::MiseToolPrepare | Self::ToolProducerReport)
    }
    /// Whether this operation belongs to an isolated native source producer.
    #[must_use]
    pub const fn is_source_producer(self) -> bool {
        matches!(
            self,
            Self::NpmPublicSourceProducer
                | Self::BunSourceProducer
                | Self::TofuProviderExport
                | Self::TofuRootOwnership
                | Self::SourceProducerReport
                | Self::GradleSourceProducer
                | Self::RustSourceProducer
        )
    }
    /// Owned auxiliary path; native execution operations return a digest directory.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::RustReportWrapper => ".github/velnor/native/rust-report/",
            Self::RustPrepareRootLinux => ".github/velnor/rust_prepare_root_linux.sh",
            Self::RustPrepareDesktopMac => ".github/velnor/rust_prepare_desktop_mac.sh",
            Self::RustPrepareDesktopSourceMac => {
                ".github/velnor/rust_prepare_desktop_source_mac.sh"
            }
            Self::RustPrepareReleaseMac => ".github/velnor/rust_prepare_release_mac.sh",
            Self::NpmPublicSourceProducer => ".github/velnor/npm_source_producer.sh",
            Self::BunSourceProducer => ".github/velnor/bun_source_producer.sh",
            Self::TofuProviderExport => ".github/velnor/tofu_provider_export.sh",
            Self::TofuRootOwnership => ".github/velnor/tofu_root_ownership.sh",
            Self::MbxExport => ".github/velnor/mbx_export.sh",
            Self::MbxProducerPrepare => ".github/velnor/mbx_producer_prepare.sh",
            Self::MbxArtifactAdmission => ".github/velnor/mbx_artifact_admission.sh",
            Self::MbxBundleVerify => ".github/velnor/mbx_bundle_verify.sh",
            Self::MbxProducerReport => ".github/velnor/mbx_producer_report.sh",
            Self::DesktopNativeHydration => ".github/velnor/desktop_native_hydration.sh",
            Self::SourceProducerReport => ".github/velnor/source_producer_report.sh",
            Self::GradleSourceProducer => ".github/velnor/gradle_source_producer.sh",
            Self::HomebrewPreparation => ".github/velnor/homebrew_preparation.sh",
            Self::PackageUpdateFixture => ".github/velnor/package_update_fixture.sh",
            Self::MiseToolPrepare => ".github/velnor/mise_tool_prepare.sh",
            Self::ToolProducerReport => ".github/velnor/tool_producer_report.sh",
            Self::MiseBootstrap => ".github/velnor/mise_bootstrap.sh",
            Self::JavaHomeMaterialize => ".github/velnor/java_home_materialize.sh",
            Self::NativePagesAdmission => ".github/velnor/native_pages_admission.sh",
            Self::NativePagesToolPreparation => ".github/velnor/native_pages_tool_preparation.sh",
            Self::RustSourceProducer => ".github/velnor/rust_source_producer.sh",
            Self::TofuCachedInit => ".github/velnor/tofu_cached_init.sh",
            Self::NativePublishAdmission => ".github/velnor/native_publish_admission.sh",
            Self::NativePublishReceiptVerifier => ".github/velnor/native_publish_receipt.sh",
            Self::NativePublishToolPreparation => ".github/velnor/native_publish_tools.sh",
            Self::NativeSwiftExecution => ".github/velnor/native/swift/",
            Self::NativeRustExecution => ".github/velnor/native/rust/",
            Self::OciDelivery => ".github/velnor/oci_delivery.sh",
            Self::AptVerify => ".github/velnor/apt_verify.sh",
            Self::AptStage => ".github/velnor/apt_stage.sh",
            Self::AptTransportIncoming => ".github/velnor/apt_transport_incoming.sh",
            Self::AptResult => ".github/velnor/apt_result.sh",
            Self::VerificationObserver => ".github/velnor/verification_observer.sh",
            Self::RustReleaseSourceSnapshot => ".github/velnor/release_source_snapshot.sh",
            Self::RustReleasePreparedPackage => ".github/velnor/release_prepared_package.sh",
            Self::RustReleasePackageVerify => ".github/velnor/release_package_verify.sh",
            Self::ReceiptOwnedPreparation => ".github/velnor/receipt_owned_preparation.sh",
            Self::ReleaseAdmissionDefaultBranch => {
                ".github/velnor/release_admission_default_branch.sh"
            }
            Self::RustReleaseAnonymousPackage => ".github/velnor/release_package.sh",
            Self::RustReleasePrepareAnonymous => ".github/velnor/release_prepare_anonymous.sh",
            Self::RustReleasePrepareTools => ".github/velnor/release_prepare_tools.sh",
            Self::RustReleasePrepareForge => ".github/velnor/release_prepare_forge.sh",
            Self::RustReleaseForgePreflight => ".github/velnor/release_forge_preflight.sh",
            Self::RustRegistryArtifactProof => ".github/velnor/release_registry_artifact_proof.sh",
            Self::RustRegistryPublish => ".github/velnor/release_registry_publish.sh",
            Self::RustForgePublish => ".github/velnor/release_forge_publish.sh",
            Self::RustReleaseReconcile => ".github/velnor/release_reconcile.sh",
            Self::CacheSnapshot => ".github/velnor/cache_snapshot.sh",
            Self::CacheReceiptManifest => ".github/velnor/cache_receipt_manifest.sh",
            Self::CacheReceiptVerify => ".github/velnor/cache_receipt_verify.sh",
            Self::CacheReceiptBundle => ".github/velnor/cache_receipt_bundle.sh",
        }
    }
}
