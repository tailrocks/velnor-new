//! Declared `.github` outputs and preserved repository inputs (gen §0).
//!
//! Generated outputs are declared here before support. The preserved-input
//! list is separate because those bytes remain repository-owned.

/// One declared generated format: exact path plus owning adapter crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GithubFormat {
    /// Exact repo-relative output path.
    pub path: &'static str,
    /// Crate that owns the format's bytes.
    pub owner: &'static str,
}

/// Generated AGENTS.md instruction path inside the repository.
pub const AGENTS_MD_PATH: &str = ".github/AGENTS.md";
/// Generated CLAUDE.md symlink path inside the repository.
pub const CLAUDE_MD_PATH: &str = ".github/CLAUDE.md";
/// Relative symlink target for sibling CLAUDE.md.
pub const CLAUDE_MD_TARGET: &str = "AGENTS.md";
/// Exact repository-owned GitHub input retained by generation.
pub const PULL_REQUEST_TEMPLATE_PATH: &str = ".github/PULL_REQUEST_TEMPLATE.md";
/// Maximum size of a preserved GitHub input (64 KiB).
pub const MAX_PRESERVED_GITHUB_INPUT_BYTES: u64 = 65_536;
/// Exact `.github` inputs that generation may carry forward unchanged.
pub const PRESERVED_GITHUB_INPUTS: [&str; 1] = [PULL_REQUEST_TEMPLATE_PATH];

/// Every declared `.github` output format (gen §0).
pub const DECLARED_GITHUB_FORMATS: [GithubFormat; 4] = [
    GithubFormat {
        path: AGENTS_MD_PATH,
        owner: "velnor-actions-workflow-renderer",
    },
    GithubFormat {
        path: CLAUDE_MD_PATH,
        owner: "velnor-actions-workflow-renderer",
    },
    GithubFormat {
        path: ".github/actionlint.yaml",
        owner: "velnor-actions-actionlint",
    },
    GithubFormat {
        path: ".github/workflows/ci.yml",
        owner: "velnor-actions-workflow-renderer",
    },
];

/// Look up the declared format for an exact output path.
#[must_use]
pub fn find_github_format(path: &str) -> Option<&'static GithubFormat> {
    DECLARED_GITHUB_FORMATS
        .iter()
        .find(|format| format.path == path)
}

/// Whether an exact output path is a declared format.
#[must_use]
pub fn is_declared_github_format(path: &str) -> bool {
    find_github_format(path).is_some()
}
