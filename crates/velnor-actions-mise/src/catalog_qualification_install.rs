//! Immutable launch closure, source lineage and measured installer transforms.

/// Kind of file whose bytes are admitted before a native launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualifiedLaunchKind {
    /// Native executable.
    Executable,
    /// Interpreter input such as the bundled npm CLI.
    Script,
    /// Java launcher archive.
    JavaArchive,
}

/// One source-archive file and its measured installed location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedLaunchEntry {
    pub(super) archive_member: &'static str,
    pub(super) installed_relative_path: Option<&'static str>,
    pub(super) sha256: &'static str,
    pub(super) kind: QualifiedLaunchKind,
}

impl QualifiedLaunchEntry {
    /// Exact source archive member.
    #[must_use]
    pub const fn archive_member(&self) -> &'static str {
        self.archive_member
    }
    /// Measured path relative to the owned Mise data directory.
    #[must_use]
    pub const fn installed_relative_path(&self) -> Option<&'static str> {
        self.installed_relative_path
    }
    /// Expected launch input SHA256.
    #[must_use]
    pub const fn sha256(&self) -> &'static str {
        self.sha256
    }
    /// Execution role of these bytes.
    #[must_use]
    pub const fn kind(&self) -> QualifiedLaunchKind {
        self.kind
    }
}

/// Additional immutable source identity, separate from the distribution provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedSourceLineage {
    pub(super) name: &'static str,
    pub(super) repository: &'static str,
    pub(super) commit: &'static str,
    pub(super) tree: &'static str,
    pub(super) version: &'static str,
}

impl QualifiedSourceLineage {
    /// Source role, for example `CPython` or bundled npm.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }
    /// Repository containing that exact source.
    #[must_use]
    pub const fn repository(&self) -> &'static str {
        self.repository
    }
    /// Exact source commit.
    #[must_use]
    pub const fn commit(&self) -> &'static str {
        self.commit
    }
    /// Exact source tree.
    #[must_use]
    pub const fn tree(&self) -> &'static str {
        self.tree
    }
    /// Exact source version or immutable provider release identifier.
    #[must_use]
    pub const fn version(&self) -> &'static str {
        self.version
    }
}

/// Installer capable of enforcing qualified artifact identity before target execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualifiedInstallBackend {
    /// Supported checksum-bound Mise HTTP archive installation.
    MiseHttp,
    /// Source-bound owned bootstrap acquisition.
    SourceBoundBootstrap,
}

/// Root-derived environment path fixed by the qualified installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedInstallEnvironment {
    pub(super) name: &'static str,
    pub(super) relative_path: &'static str,
}

impl QualifiedInstallEnvironment {
    /// Fixed environment name. PATH prepends this directory; home keys set it.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }
    /// Path below the installation root; empty means that root itself.
    #[must_use]
    pub const fn relative_path(&self) -> &'static str {
        self.relative_path
    }
}

/// Measured installation layout and transforms, never inferred from a selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedInstallPlan {
    pub(super) backend: QualifiedInstallBackend,
    pub(super) strip_components: u8,
    pub(super) bin_path: &'static str,
    pub(super) root_relative_path: &'static str,
    pub(super) transform_abi: &'static str,
    pub(super) environment: &'static [QualifiedInstallEnvironment],
}

impl QualifiedInstallPlan {
    /// Qualified installer backend.
    #[must_use]
    pub const fn backend(&self) -> QualifiedInstallBackend {
        self.backend
    }
    /// Explicit archive extraction strip count.
    #[must_use]
    pub const fn strip_components(&self) -> u8 {
        self.strip_components
    }
    /// Explicit binary directory below the installation root.
    #[must_use]
    pub const fn bin_path(&self) -> &'static str {
        self.bin_path
    }
    /// Measured installation root relative to the owned Mise data directory.
    #[must_use]
    pub const fn root_relative_path(&self) -> &'static str {
        self.root_relative_path
    }
    /// Source-qualified transformation ABI and options identity.
    #[must_use]
    pub const fn transform_abi(&self) -> &'static str {
        self.transform_abi
    }
    /// Environment paths derived only from the installation root.
    #[must_use]
    pub const fn environment(&self) -> &'static [QualifiedInstallEnvironment] {
        self.environment
    }
}
