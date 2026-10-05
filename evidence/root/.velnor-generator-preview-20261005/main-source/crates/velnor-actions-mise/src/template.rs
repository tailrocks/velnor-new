//! Logical Rust task templates: stable names and arity, no task files.
//!
//! V1 never writes `.mise/tasks` files. These logical definitions pair each
//! focused template name with its package/config arity; execution runs
//! through pinned Mise, and the typed output preserves the real exit status.
//! `dependencies` is repository-wide; every other template targets one
//! package/configuration.

/// Focused task template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskTemplate {
    /// Formatting validation once for the selected source tree.
    FmtCheck,
    /// Repository-wide dependency policy checks.
    Dependencies,
    /// Generated workflow syntax validation.
    Actionlint,
    /// Generated workflow security validation.
    Zizmor,
    /// Configured Clippy validation for one package.
    Clippy,
    /// Single build for one package when the runner needs an archive.
    TestBuild,
    /// Cargo test or the prepared Nextest configuration for one package.
    Test,
    /// Documentation tests for one package.
    Doctest,
    /// Package documentation with warnings denied.
    Doc,
    /// Minimum-Rust-version check for one package.
    Msrv,
}

impl TaskTemplate {
    /// Every template in stable order.
    pub const ALL: [Self; 10] = [
        Self::FmtCheck,
        Self::Dependencies,
        Self::Actionlint,
        Self::Zizmor,
        Self::Clippy,
        Self::TestBuild,
        Self::Test,
        Self::Doctest,
        Self::Doc,
        Self::Msrv,
    ];

    /// Stable template name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FmtCheck => "fmt-check",
            Self::Dependencies => "dependencies",
            Self::Actionlint => "actionlint",
            Self::Zizmor => "zizmor",
            Self::Clippy => "clippy",
            Self::TestBuild => "test-build",
            Self::Test => "test",
            Self::Doctest => "doctest",
            Self::Doc => "doc",
            Self::Msrv => "msrv",
        }
    }

    /// Resolve a stable name to its template.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().find(|task| task.name() == name).copied()
    }

    /// Whether the template inspects the repository instead of one package.
    ///
    /// Only `dependencies` is repository-wide: it inspects manifests
    /// without compiling every crate.
    #[must_use]
    pub const fn is_repo_wide(self) -> bool {
        matches!(self, Self::Dependencies)
    }

    /// Whether the template targets exactly one package/configuration.
    #[must_use]
    pub const fn targets_single_package(self) -> bool {
        !self.is_repo_wide()
    }
}
