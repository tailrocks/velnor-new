//! Declared `.github` output formats (gen §0).
//!
//! V1 generates exactly three files; future adapters MUST declare additional
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

/// Retired generator-owned `.github` paths, sorted.
///
/// Generation no longer emits these paths but still owns them: the
/// preserve-copy skips them so the whole-tree swap deletes stale copies
/// from consumer repositories. `.github/CLAUDE.md` retired because plugin
/// installers reject symlink entries and a second instruction copy next to
/// [`AGENTS_MD_PATH`] carries no benefit.
pub const RETIRED_GITHUB_PATHS: &[&str] = &[".github/CLAUDE.md"];

/// Every declared `.github` output format (gen §0).
pub const DECLARED_GITHUB_FORMATS: [GithubFormat; 3] = [
    GithubFormat {
        path: AGENTS_MD_PATH,
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
