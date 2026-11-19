// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{PackageName, PathGlob, TargetName};

use crate::error::Reason;

pub(crate) fn excludes(entries: Vec<String>) -> Result<Vec<PathGlob>, Reason> {
    let invalid = |value| Reason::InvalidValue {
        key: "exclude",
        value,
    };
    entries
        .into_iter()
        .map(|entry| PathGlob::new(entry.trim_end_matches('/')).map_err(|_| invalid(entry)))
        .collect()
}

pub(crate) fn required_check(
    configured: Option<String>,
    default: String,
) -> Result<String, Reason> {
    match configured {
        None => Ok(default),
        Some(name) if !name.trim().is_empty() && !name.chars().any(char::is_control) => Ok(name),
        Some(name) => Err(Reason::InvalidValue {
            key: "required_check",
            value: name,
        }),
    }
}

pub(crate) fn harnesses(entries: Vec<String>) -> Result<Vec<(PackageName, TargetName)>, Reason> {
    entries
        .into_iter()
        .map(|entry| {
            let invalid = || Reason::InvalidValue {
                key: "nextest_harnesses",
                value: entry.clone(),
            };
            let (package, target) = entry.split_once(':').ok_or_else(invalid)?;
            Ok((
                PackageName::new(package).map_err(|_| invalid())?,
                TargetName::new(target).map_err(|_| invalid())?,
            ))
        })
        .collect()
}
