// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;

use crate::error::CiError;

pub(super) fn max_age_days_from(value: Option<&OsStr>) -> Result<Option<u64>, CiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let text = value
        .to_str()
        .ok_or_else(|| CiError::new("VELNOR_BASE_MAX_AGE_DAYS is not Unicode"))?;
    Ok(text.trim().parse::<u64>().ok().filter(|days| *days > 0))
}

pub(super) fn budget_bytes_from(value: Option<&OsStr>) -> Result<Option<u64>, CiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let text = value
        .to_str()
        .ok_or_else(|| CiError::new("VELNOR_CACHE_BUDGET_BYTES is not Unicode"))?
        .trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse()
        .map(Some)
        .map_err(|_| CiError::new("VELNOR_CACHE_BUDGET_BYTES is not a number of bytes"))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    use super::{budget_bytes_from, max_age_days_from};

    #[test]
    fn a_budget_that_is_not_unicode_is_refused_not_ignored() {
        let refused = budget_bytes_from(Some(OsStr::from_bytes(b"\xff")));
        assert!(refused.is_err(), "{refused:?}");
    }

    #[test]
    fn budget_and_age_parse_numbers_and_treat_empty_as_unset() {
        assert_eq!(budget_bytes_from(Some(OsStr::new(" 10 "))).ok(), Some(Some(10)));
        assert_eq!(budget_bytes_from(Some(OsStr::new(" "))).ok(), Some(None));
        assert_eq!(budget_bytes_from(None).ok(), Some(None));
        assert_eq!(max_age_days_from(Some(OsStr::new("7"))).ok(), Some(Some(7)));
        assert_eq!(max_age_days_from(Some(OsStr::new("0"))).ok(), Some(None));
    }

    #[test]
    fn a_max_age_that_is_not_unicode_is_refused_not_ignored() {
        let refused = max_age_days_from(Some(OsStr::from_bytes(b"\xff")));
        assert!(refused.is_err(), "{refused:?}");
    }
}
