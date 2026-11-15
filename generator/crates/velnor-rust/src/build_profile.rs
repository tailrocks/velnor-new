// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::CiDebugInfo;

const NO_DEPENDENCY_DEBUG: &str = "[profile.dev.package.\"*\"]\ndebug = \"none\"\n\n[profile.test.package.\"*\"]\ndebug = \"none\"\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProfile {
    pub env: Vec<(&'static str, &'static str)>,
    pub cargo_config: &'static str,
}

#[must_use]
pub fn build_profile(debuginfo: CiDebugInfo) -> BuildProfile {
    let (workspace, dependencies) = match debuginfo {
        CiDebugInfo::Full => ("full", ""),
        CiDebugInfo::LineTablesOnly => ("line-tables-only", NO_DEPENDENCY_DEBUG),
        CiDebugInfo::None => ("none", NO_DEPENDENCY_DEBUG),
    };
    BuildProfile {
        env: vec![
            ("CARGO_INCREMENTAL", "0"),
            ("CARGO_TERM_COLOR", "always"),
            ("SOURCE_DATE_EPOCH", "0"),
            ("CARGO_PROFILE_DEV_DEBUG", workspace),
            ("CARGO_PROFILE_TEST_DEBUG", workspace),
        ],
        cargo_config: dependencies,
    }
}
