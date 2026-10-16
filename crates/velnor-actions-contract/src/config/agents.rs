//! Agent instruction file policy (`[agents]`).

use serde::{Deserialize, Serialize};

/// Emission mode for `.github/CLAUDE.md`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeMdMode {
    /// Emit a regular file containing `@AGENTS.md\n` (portable default).
    #[default]
    Import,
    /// Emit a relative symlink pointing to `AGENTS.md`.
    Symlink,
}

impl ClaudeMdMode {
    /// True when the mode is the portable `import` default.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Import)
    }

    /// Canonical config spelling of this mode.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Symlink => "symlink",
        }
    }
}

/// Agent instruction configuration (`[agents]` table).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentsConfig {
    /// How `.github/CLAUDE.md` is emitted and checked for drift.
    #[serde(default, skip_serializing_if = "ClaudeMdMode::is_default")]
    pub claude_md: ClaudeMdMode,
}

impl AgentsConfig {
    /// True when all fields match their defaults.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.claude_md.is_default()
    }
}
