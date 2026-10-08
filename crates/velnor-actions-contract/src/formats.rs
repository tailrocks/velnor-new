//! Declared `.github` output formats (gen §0).
//!
//! V1 generates exactly two files; future adapters MUST declare additional
//! formats here before they are supported. Writers consult this registry and
//! MUST NOT emit undeclared paths.

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
/// Generated CLAUDE.md pointer path inside the repository: a regular file
/// with the single [`CLAUDE_MD_POINTER_BODY`] import line, never a symlink.
pub const CLAUDE_MD_PATH: &str = ".github/CLAUDE.md";
/// Exact bytes of every generated `CLAUDE.md`: one `@AGENTS.md` import line
/// with a trailing newline. Claude resolves the sibling instructions through
/// the import instead of symlink resolution, which plugin installers and
/// Windows checkouts cannot carry reliably.
pub const CLAUDE_MD_POINTER_BODY: &str = "@AGENTS.md\n";

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
