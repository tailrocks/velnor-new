// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskTemplate {
    FmtCheck,
    Dependencies,
    Jactionlint,
    Clippy,
    TestBuild,
    Test,
    Doctest,
    Doc,
    Msrv,
}

impl TaskTemplate {
    pub const ALL: [Self; 9] = [
        Self::FmtCheck,
        Self::Dependencies,
        Self::Jactionlint,
        Self::Clippy,
        Self::TestBuild,
        Self::Test,
        Self::Doctest,
        Self::Doc,
        Self::Msrv,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FmtCheck => "fmt-check",
            Self::Dependencies => "dependencies",
            Self::Jactionlint => "jactionlint",
            Self::Clippy => "clippy",
            Self::TestBuild => "test-build",
            Self::Test => "test",
            Self::Doctest => "doctest",
            Self::Doc => "doc",
            Self::Msrv => "msrv",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().find(|task| task.name() == name).copied()
    }

    #[must_use]
    pub const fn is_repo_wide(self) -> bool {
        matches!(self, Self::Dependencies)
    }

    #[must_use]
    pub const fn targets_single_package(self) -> bool {
        !self.is_repo_wide()
    }
}
