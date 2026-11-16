// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::ReleaseError;
use crate::tag::TagPattern;

pub(crate) fn check_effective_tags(tags: &[(String, TagPattern)]) -> Result<(), ReleaseError> {
    for (index, (name, pattern)) in tags.iter().enumerate() {
        for (other_name, other) in &tags[index + 1..] {
            if name == other_name {
                continue;
            }
            if pattern == other {
                return Err(ReleaseError::DuplicateTagPattern {
                    pattern: pattern.as_str().to_owned(),
                    packages: vec![name.clone(), other_name.clone()],
                });
            }
            if pattern.may_collide(other) {
                return Err(ReleaseError::TagsMayCollide {
                    first: pattern.as_str().to_owned(),
                    second: other.as_str().to_owned(),
                });
            }
        }
    }
    Ok(())
}
