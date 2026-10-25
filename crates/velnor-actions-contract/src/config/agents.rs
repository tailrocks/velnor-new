use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeMdMode {
    #[default]
    Import,

    Symlink,
}

impl ClaudeMdMode {
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Import)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Symlink => "symlink",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentsConfig {
    #[serde(default, skip_serializing_if = "ClaudeMdMode::is_default")]
    pub claude_md: ClaudeMdMode,
}

impl AgentsConfig {
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.claude_md.is_default()
    }
}
