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

/// Every declared `.github` output format (gen §0).
pub const DECLARED_GITHUB_FORMATS: [GithubFormat; 2] = [
    GithubFormat {
        path: ".github/actionlint.yaml",
        owner: "velnor-actions-actionlint",
    },
    GithubFormat {
        path: ".github/workflows/velnor.yml",
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
