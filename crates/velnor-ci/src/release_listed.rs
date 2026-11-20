// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::publish::{SUMS, read_sums};

#[derive(Debug, PartialEq, Eq)]
pub enum Listing {
    Listed,
    Unlisted,
}

pub fn listing(sums: &[u8], name: &str, present: bool) -> Result<Listing, CiError> {
    if !read_sums(sums)?.contains_key(name) {
        return Ok(Listing::Unlisted);
    }
    if present {
        Ok(Listing::Listed)
    } else {
        Err(CiError::new(format!(
            "{SUMS} lists {name}, which is not in the directory"
        )))
    }
}
