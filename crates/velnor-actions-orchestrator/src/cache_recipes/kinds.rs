// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheRecipeKind {
    Native,
    Preview,
    Stable,
    Helper,
}

impl CacheRecipeKind {
    pub const ALL: [Self; 4] = [Self::Native, Self::Preview, Self::Stable, Self::Helper];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Preview => "preview",
            Self::Stable => "stable",
            Self::Helper => "helper",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheRecipe {
    pub recipe_key: &'static str,
    pub kind: CacheRecipeKind,
    pub command_family: &'static str,
    pub command_builder_id: &'static str,
    pub command_argv: Option<&'static [&'static str]>,
    pub tool_route: &'static str,
    pub package: &'static str,
    pub cargo_profile: &'static str,
    pub nextest_profile: &'static str,
    pub targets: &'static [&'static str],
    pub features: &'static [&'static str],
    pub default_features: bool,
    pub lane_id: &'static str,
    pub source_inputs: &'static [&'static str],
    pub environment_inputs: &'static [&'static str],
    pub output_roles: &'static [&'static str],
    pub source_bound_helper: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheRecipeError {
    pub recipe_key: &'static str,
    pub problem: &'static str,
}

impl fmt::Display for CacheRecipeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cache recipe {} is invalid: {}",
            self.recipe_key, self.problem
        )
    }
}

impl std::error::Error for CacheRecipeError {}

#[must_use]
pub fn parse_kind(value: &str) -> Option<CacheRecipeKind> {
    CacheRecipeKind::ALL
        .into_iter()
        .find(|kind| kind.as_str() == value)
}
