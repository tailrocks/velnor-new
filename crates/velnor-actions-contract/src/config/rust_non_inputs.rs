// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::discover::glob_may_overlap;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const NON_INPUTS_FIELD: &str = "stacks.rust.non_inputs";

const SPARSE_INCLUDE_ALL: &str = "/*";

const DIRECTORY_TAIL: &str = "/**";

const ANY_DIRECTORY: &str = "*";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NonInputs(Vec<String>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    Exact,
    Folded,
}

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
        self.0.iter().any(|glob| non_input_covers(glob, path))
    }

    #[must_use]
    pub fn contains_ignoring_case(&self, path: &str) -> bool {
        self.0.iter().any(|glob| covers(glob, path, Case::Folded))
    }

    #[must_use]
    pub fn sparse_checkout_patterns(&self) -> Vec<String> {
        if self.0.is_empty() {
            return Vec::new();
        }
        let mut patterns = vec![SPARSE_INCLUDE_ALL.to_owned()];
        patterns.extend(self.0.iter().map(|glob| format!("!/{glob}")));
        patterns
    }

    #[must_use]
    pub fn pathspecs(&self) -> Vec<String> {
        self.0.iter().map(|glob| format!(":(glob){glob}")).collect()
    }

    pub(super) fn validate(&self, file: &str) -> Result<(), ContractError> {
        let mut seen = BTreeSet::new();
        for glob in &self.0 {
            if directory_prefix(glob).is_none() {
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

fn directory_prefix(glob: &str) -> Option<Vec<&str>> {
    let segments: Vec<&str> = glob.strip_suffix(DIRECTORY_TAIL)?.split('/').collect();
    segments
        .iter()
        .all(|segment| is_segment(segment))
        .then_some(segments)
}

fn is_segment(segment: &str) -> bool {
    segment == ANY_DIRECTORY
        || (segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            && segment.bytes().any(|byte| byte != b'.'))
}

fn segment_matches(want: &str, got: &str, case: Case) -> bool {
    match case {
        Case::Exact => want == got,
        Case::Folded => want.eq_ignore_ascii_case(got),
    }
}

fn covers(glob: &str, path: &str, case: Case) -> bool {
    let Some(prefix) = directory_prefix(glob) else {
        return false;
    };
    let mut parts = path.split('/');
    prefix.iter().all(|want| {
        parts.next().is_some_and(|got| {
            !got.is_empty() && (*want == ANY_DIRECTORY || segment_matches(want, got, case))
        })
    }) && parts.next().is_some()
}

#[must_use]
pub fn non_input_covers(glob: &str, path: &str) -> bool {
    covers(glob, path, Case::Exact)
}

#[must_use]
pub fn non_input_may_overlap(glob: &str, other: &str) -> bool {
    directory_prefix(glob)
        .is_none_or(|prefix| glob_may_overlap(&format!("{}/*/**", prefix.join("/")), other))
}

#[must_use]
pub fn non_input_may_reach_tree(glob: &str, dir: &str) -> bool {
    let Some(prefix) = directory_prefix(glob) else {
        return true;
    };
    dir.is_empty()
        || prefix
            .iter()
            .zip(dir.split('/'))
            .all(|(want, got)| *want == ANY_DIRECTORY || *want == got)
}

#[must_use]
pub fn is_sparse_checkout_exclusion(patterns: &str) -> bool {
    let mut lines = patterns.lines();
    lines.next() == Some(SPARSE_INCLUDE_ALL)
        && lines.clone().next().is_some()
        && lines.all(|line| line.starts_with("!/") && line.len() > 2)
}
