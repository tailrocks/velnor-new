// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::CiDebugInfo;

const FIRST_STRING_DEBUG: (u32, u32) = (1, 71);
const NO_DEPENDENCY_DEBUG_NUMERIC: &str =
    "[profile.dev.package.\"*\"]\ndebug = 0\n\n[profile.test.package.\"*\"]\ndebug = 0\n";
const NO_DEPENDENCY_DEBUG: &str = "[profile.dev.package.\"*\"]\ndebug = \"none\"\n\n[profile.test.package.\"*\"]\ndebug = \"none\"\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProfile {
    pub env: Vec<(&'static str, &'static str)>,
    pub cargo_config: &'static str,
}

#[must_use]
pub fn rustc_release(verbose: &str) -> Option<(u32, u32)> {
    let release = verbose
        .lines()
        .find_map(|line| line.strip_prefix("release: "))?;
    let mut parts = release.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.split('-').next()?.parse().ok()?;
    Some((major, minor))
}

#[must_use]
pub fn build_profile(debuginfo: CiDebugInfo, rustc: (u32, u32)) -> BuildProfile {
    let (workspace, dependencies) = if rustc >= FIRST_STRING_DEBUG {
        match debuginfo {
            CiDebugInfo::Full => ("full", ""),
            CiDebugInfo::LineTablesOnly => ("line-tables-only", NO_DEPENDENCY_DEBUG),
            CiDebugInfo::None => ("none", NO_DEPENDENCY_DEBUG),
        }
    } else {
        match debuginfo {
            CiDebugInfo::Full => ("2", ""),
            CiDebugInfo::LineTablesOnly => ("1", NO_DEPENDENCY_DEBUG_NUMERIC),
            CiDebugInfo::None => ("0", NO_DEPENDENCY_DEBUG_NUMERIC),
        }
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
