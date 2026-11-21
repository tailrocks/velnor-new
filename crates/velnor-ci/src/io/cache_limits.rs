// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;

use crate::error::CiError;

const MAX_AGE_VARIABLE: &str = "VELNOR_BASE_MAX_AGE_DAYS";
const BUDGET_VARIABLE: &str = "VELNOR_CACHE_BUDGET_BYTES";

fn text_from<'a>(variable: &str, value: Option<&'a OsStr>) -> Result<Option<&'a str>, CiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let text = value
        .to_str()
        .ok_or_else(|| {
            CiError::new(format!(
                "{variable} is not Unicode: {}",
                value.as_encoded_bytes().escape_ascii()
            ))
        })?
        .trim();
    Ok((!text.is_empty()).then_some(text))
}

pub(super) fn max_age_days_from(value: Option<&OsStr>) -> Result<Option<u64>, CiError> {
    let Some(text) = text_from(MAX_AGE_VARIABLE, value)? else {
        return Ok(None);
    };
    match text.parse::<u64>() {
        Ok(0) => Ok(None),
        Ok(days) => Ok(Some(days)),
        Err(_) => Err(CiError::new(format!(
            "{MAX_AGE_VARIABLE} must be a whole number of days (0 turns the limit off), got {text:?}"
        ))),
    }
}

pub(super) fn budget_bytes_from(value: Option<&OsStr>) -> Result<Option<u64>, CiError> {
    let Some(text) = text_from(BUDGET_VARIABLE, value)? else {
        return Ok(None);
    };
    text.parse::<u64>().map(Some).map_err(|_| {
        CiError::new(format!(
            "{BUDGET_VARIABLE} must be a whole number of bytes, got {text:?}"
        ))
    })
}
