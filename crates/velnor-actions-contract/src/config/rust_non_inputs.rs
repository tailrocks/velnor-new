// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::discover::{expand_braces, glob_may_overlap, matches_glob, validate_pattern};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const NON_INPUTS_FIELD: &str = "stacks.rust.non_inputs";

const SPARSE_INCLUDE_ALL: &str = "/*";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NonInputs(Vec<String>);

impl NonInputs {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn globs(&self) -> &[String] {
        &self.0
    }

    #[must_use]
    pub fn contains(&self, path: &str) -> bool {
        self.0.iter().any(|glob| matches_glob(glob, path))
    }

    #[must_use]
    pub fn sparse_checkout_patterns(&self) -> Vec<String> {
        if self.0.is_empty() {
            return Vec::new();
        }
        let mut patterns = vec![SPARSE_INCLUDE_ALL.to_owned()];
        patterns.extend(
            self.0
                .iter()
                .flat_map(|glob| expand_braces(glob))
                .map(|glob| format!("!/{glob}")),
        );
        patterns
    }

    #[must_use]
    pub fn pathspecs(&self) -> Vec<String> {
        self.0
            .iter()
            .flat_map(|glob| expand_braces(glob))
            .map(|glob| format!(":(glob){glob}"))
            .collect()
    }

    pub(super) fn validate(&self, file: &str) -> Result<(), ContractError> {
        let mut seen = BTreeSet::new();
        for glob in &self.0 {
            if validate_pattern(glob).is_err() || glob.starts_with('!') {
                return Err(ContractError::config(
                    file,
                    NON_INPUTS_FIELD,
                    format!("bad_glob:{glob}"),
                ));
            }
            if !seen.insert(glob.as_str()) {
                return Err(ContractError::config(
                    file,
                    NON_INPUTS_FIELD,
                    format!("duplicate_glob:{glob}"),
                ));
            }
        }
        Ok(())
    }
}

#[must_use]
pub fn is_sparse_checkout_exclusion(patterns: &str) -> bool {
    let mut lines = patterns.lines();
    lines.next() == Some(SPARSE_INCLUDE_ALL)
        && lines.clone().next().is_some()
        && lines.all(|line| line.starts_with("!/") && line.len() > 2)
}

#[must_use]
pub fn glob_may_reach_tree(glob: &str, dir: &str) -> bool {
    dir.is_empty() || glob_may_overlap(glob, &format!("{dir}/**"))
}
