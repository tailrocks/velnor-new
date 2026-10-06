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
/// Generated CLAUDE.md symlink path inside the repository.
pub const CLAUDE_MD_PATH: &str = ".github/CLAUDE.md";
/// Relative symlink target for sibling CLAUDE.md.
pub const CLAUDE_MD_TARGET: &str = "AGENTS.md";
/// Repository-owned PR template path: preserved byte-for-byte, never generated.
///
/// The only `.github` file `generate` carries over instead of replacing
/// (generated-file-contract §3): generation never writes this path, so an
/// existing template survives and a missing one stays missing. Preserved
/// bytes skip the generated marker and token gates (GitHub renders the
/// template; it never executes), but path, size, and UTF-8 gates apply.
pub const PULL_REQUEST_TEMPLATE_PATH: &str = ".github/PULL_REQUEST_TEMPLATE.md";

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
