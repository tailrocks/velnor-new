#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GithubFormat {
    pub path: &'static str,

    pub owner: &'static str,
}

pub const AGENTS_MD_PATH: &str = ".github/AGENTS.md";

pub const CLAUDE_MD_PATH: &str = ".github/CLAUDE.md";

pub const CLAUDE_MD_POINTER_BODY: &str = "@AGENTS.md\n";

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
        path: ".github/jactionlint.yaml",
        owner: "velnor-actions-actionlint",
    },
    GithubFormat {
        path: ".github/workflows/ci.yml",
        owner: "velnor-actions-workflow-renderer",
    },
];

#[must_use]
pub fn find_github_format(path: &str) -> Option<&'static GithubFormat> {
    DECLARED_GITHUB_FORMATS
        .iter()
        .find(|format| format.path == path)
}

#[must_use]
pub fn is_declared_github_format(path: &str) -> bool {
    find_github_format(path).is_some()
}
