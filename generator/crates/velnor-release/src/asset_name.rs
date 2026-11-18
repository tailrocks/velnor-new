// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::ReleaseError;
use crate::tag::TagPattern;
use crate::velnor_config::{Binary, BinaryFormat};

const BIN: &str = "{{ bin }}";
const PLATFORM: &str = "{{ platform }}";
const TARGET: &str = "{{ target }}";
const VERSION: &str = "{{ version }}";

pub(crate) fn valid(template: &str) -> bool {
    let literal = [BIN, PLATFORM, TARGET, VERSION]
        .iter()
        .fold(template.to_owned(), |text, name| text.replace(name, "x"));
    literal.starts_with(|c: char| c.is_ascii_alphanumeric())
        && literal
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && (template.contains(PLATFORM) || template.contains(TARGET))
}

#[must_use]
pub fn asset_name_uses_version(template: &str) -> bool {
    template.contains(VERSION)
}

#[must_use]
pub fn expand_asset_name(
    template: &str,
    bin: &str,
    platform: &str,
    target: &str,
    version: &str,
) -> String {
    template
        .replace(BIN, bin)
        .replace(PLATFORM, platform)
        .replace(TARGET, target)
        .replace(VERSION, version)
}

pub(crate) fn checked(binary: &Binary, tag: &TagPattern) -> Result<Option<String>, ReleaseError> {
    match &binary.asset_name {
        Some(template) if binary.format == BinaryFormat::Raw => {
            Err(ReleaseError::InvalidAssetName {
                package: binary.package.clone(),
                template: template.clone(),
            })
        }
        Some(template) if asset_name_uses_version(template) && !tag.ends_with_version() => {
            Err(ReleaseError::InvalidAssetName {
                package: binary.package.clone(),
                template: template.clone(),
            })
        }
        other => Ok(other.clone()),
    }
}
