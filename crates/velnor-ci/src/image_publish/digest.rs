// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::image_candidate::json::is_hex;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest(String);

impl Digest {
    pub fn parse(text: &str) -> Result<Self, CiError> {
        match text.strip_prefix("sha256:") {
            Some(hex) if is_hex(hex, 64) => Ok(Self(text.to_owned())),
            _ => Err(CiError::new(
                "Registry objects are addressed by a sha256 digest only; a tag or any other reference is refused.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn hex(&self) -> &str {
        &self.0["sha256:".len()..]
    }
}
