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

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    use super::{budget_bytes_from, max_age_days_from};

    #[test]
    fn a_budget_that_is_not_unicode_names_the_variable() {
        assert_eq!(
            budget_bytes_from(Some(OsStr::from_bytes(b"\xff")))
                .expect_err("refused")
                .to_string(),
            r"VELNOR_CACHE_BUDGET_BYTES is not Unicode: \xff"
        );
    }

    #[test]
    fn a_max_age_that_is_not_unicode_names_the_variable() {
        assert_eq!(
            max_age_days_from(Some(OsStr::from_bytes(b"\xff")))
                .expect_err("refused")
                .to_string(),
            r"VELNOR_BASE_MAX_AGE_DAYS is not Unicode: \xff"
        );
    }

    #[test]
    fn a_malformed_max_age_is_refused_and_names_the_variable_and_value() {
        for value in ["seven", "-1", "1.5", "99999999999999999999999"] {
            assert_eq!(
                max_age_days_from(Some(OsStr::new(value)))
                    .expect_err("refused")
                    .to_string(),
                format!(
                    "VELNOR_BASE_MAX_AGE_DAYS must be a whole number of days (0 turns the limit off), got {value:?}"
                ),
            );
        }
    }

    #[test]
    fn a_malformed_budget_is_refused_and_names_the_variable_and_value() {
        for value in ["eight", "-1", "1.5", "99999999999999999999999"] {
            assert_eq!(
                budget_bytes_from(Some(OsStr::new(value)))
                    .expect_err("refused")
                    .to_string(),
                format!("VELNOR_CACHE_BUDGET_BYTES must be a whole number of bytes, got {value:?}"),
            );
        }
    }

    #[test]
    fn numbers_parse_and_empty_or_unset_means_no_limit() {
        assert_eq!(
            budget_bytes_from(Some(OsStr::new(" 10 "))).ok(),
            Some(Some(10))
        );
        assert_eq!(budget_bytes_from(Some(OsStr::new(" "))).ok(), Some(None));
        assert_eq!(budget_bytes_from(None).ok(), Some(None));
        assert_eq!(max_age_days_from(Some(OsStr::new("7"))).ok(), Some(Some(7)));
        assert_eq!(
            max_age_days_from(Some(OsStr::new(" 7 "))).ok(),
            Some(Some(7))
        );
        assert_eq!(max_age_days_from(Some(OsStr::new("0"))).ok(), Some(None));
        assert_eq!(max_age_days_from(Some(OsStr::new(""))).ok(), Some(None));
        assert_eq!(max_age_days_from(None).ok(), Some(None));
    }
}
