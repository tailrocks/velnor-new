//! Closed tool, host, container and requirement identities.

/// Tool requiring qualified installed bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionTool {
    /// Stock release coordinator CLI, separate from the owned publisher.
    ReleasePlz,
    /// Exact publisher API compatibility validator.
    CargoSemverChecks,
    /// Source-bound GitHub receipt bootstrap.
    Gh,
    /// Mise executable.
    Mise,
    /// Mr Boxington executable.
    Mbx,
    /// Bun runtime and package manager.
    Bun,
    /// Node runtime with its bundled npm launch closure.
    Node,
    /// OpenTofu executable.
    OpenTofu,
    /// CPython executable from its qualified provider distribution.
    Python,
    /// uv package manager.
    Uv,
    /// Community GraalVM Java distribution.
    Java,
    /// Gradle distribution and launcher closure.
    Gradle,
}

/// Qualified asset container; installers never infer this from a URL suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionAssetFormat {
    /// Downloaded bytes are the executable itself.
    Binary,
    /// Gzip-compressed tar archive with one explicitly admitted executable member.
    TarGzip,
    /// XZ-compressed tar archive.
    TarXz,
    /// ZIP archive.
    Zip,
}

/// Closed hosts whose published artifact bytes can be qualified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionHost {
    /// GNU Linux on AMD64.
    LinuxAmd64,
    /// GNU Linux on ARM64.
    LinuxArm64,
    /// Darwin on ARM64.
    MacosArm64,
}

impl DistributionHost {
    /// Exact supported target triple; unknown targets never select a default host.
    #[must_use]
    pub fn for_target(target: &str) -> Option<Self> {
        match target {
            "x86_64-unknown-linux-gnu" => Some(Self::LinuxAmd64),
            "aarch64-unknown-linux-gnu" => Some(Self::LinuxArm64),
            "aarch64-apple-darwin" => Some(Self::MacosArm64),
            _ => None,
        }
    }

    /// Exact distribution ABI.
    #[must_use]
    pub const fn abi(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "x86_64-unknown-linux-gnu",
            Self::LinuxArm64 => "aarch64-unknown-linux-gnu",
            Self::MacosArm64 => "aarch64-apple-darwin",
        }
    }
}

/// Behavior qualified at the distribution's source identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisioningMode {
    /// Official upstream behavior; cannot satisfy owned behavior requirements.
    Official,
    /// Mise excludes `.miserc` and consumes only the admitted configuration.
    MiseNoMisercExclusiveConfig,
    /// MBX preserves useful state and isolates its cache ownership.
    MbxIsolatedUsefulState,
}

/// Closed generator behavior requirement, selected by product code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionRequirement {
    /// Mise excludes `.miserc` and admits only explicitly owned configuration.
    RequiresNoMiserc,
    /// MBX uses the isolated useful-state transport.
    MbxTransport,
}

impl DistributionRequirement {
    pub(super) const fn tool(self) -> DistributionTool {
        match self {
            Self::RequiresNoMiserc => DistributionTool::Mise,
            Self::MbxTransport => DistributionTool::Mbx,
        }
    }

    pub(super) const fn provisioning_mode(self) -> ProvisioningMode {
        match self {
            Self::RequiresNoMiserc => ProvisioningMode::MiseNoMisercExclusiveConfig,
            Self::MbxTransport => ProvisioningMode::MbxIsolatedUsefulState,
        }
    }
}
