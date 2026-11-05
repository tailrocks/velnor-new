// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use crate::errors::ContractError;

use super::family::{CacheFamily, Ceiling, GIB, MIB};
use super::state::RootSlug;

pub const SLICE_ID_MAX_BYTES: usize = 48;

pub const SLICE_CEILING: Ceiling = Ceiling::StoredAndExtracted {
    stored: 768 * MIB,
    extracted: 6 * GIB,
};

const SLICE_TOKEN: &str = "slice-";
const CLI_TOKEN: &str = "cli";
const SUITE_TOKEN: &str = "suite-";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SliceId(pub(super) String);

impl SliceId {
    pub fn parse(text: &str) -> Result<Self, ContractError> {
        let shaped = !text.is_empty()
            && text.len() <= SLICE_ID_MAX_BYTES
            && text.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !shaped {
            return Err(ContractError::identity(
                "state.slice",
                format!("slice_not_shaped:{text}"),
            ));
        }
        if format!("-{text}-").contains("-s256-") {
            return Err(ContractError::identity(
                "state.slice",
                format!("slice_collides_with_digest_delimiter:{text}"),
            ));
        }
        Ok(Self(text.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SliceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StateUnit {
    Slice(SliceId),
    Cli,
    Suite(RootSlug),
}

impl StateUnit {
    #[must_use]
    pub fn token(&self) -> String {
        match self {
            Self::Slice(id) => format!("{SLICE_TOKEN}{id}"),
            Self::Cli => CLI_TOKEN.to_owned(),
            Self::Suite(root) => format!("{SUITE_TOKEN}{root}"),
        }
    }

    #[must_use]
    pub fn output_token(&self) -> String {
        self.token().replace('-', "_")
    }

    #[must_use]
    pub const fn ceiling(&self) -> Ceiling {
        match self {
            Self::Slice(_) => SLICE_CEILING,
            Self::Cli | Self::Suite(_) => CacheFamily::MbxBundle.spec().ceiling,
        }
    }

    #[must_use]
    pub const fn is_attested(&self) -> bool {
        !matches!(self, Self::Suite(_))
    }
}

impl fmt::Display for StateUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.token())
    }
}

#[must_use]
pub fn is_state_output_token(token: &str) -> bool {
    let unit = token.replace('_', "-");
    let named = |rest: &str| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    unit == CLI_TOKEN
        || [SLICE_TOKEN, SUITE_TOKEN]
            .iter()
            .any(|kind| unit.strip_prefix(kind).is_some_and(named))
}
