// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::json::{Res, require};

/// A GitHub account login: 1 to 39 ASCII letters, digits and hyphens, starting with a
/// letter or digit, with no `--`. It is the only part of a `/users/<login>` request that
/// does not come from this crate, so it is parsed before it is put in a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login(String);

impl Login {
    /// Fails closed on anything that is not a login.
    pub fn parse(text: &str) -> Res<Self> {
        let bytes = text.as_bytes();
        require(
            (1..=39).contains(&bytes.len())
                && bytes[0].is_ascii_alphanumeric()
                && bytes
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
                && !text.contains("--"),
        )?;
        Ok(Self(text.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The one account path a source preview check reads outside the repository.
    #[must_use]
    pub fn path(&self) -> String {
        format!("/users/{}", self.0)
    }
}
