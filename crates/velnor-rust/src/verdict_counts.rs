// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Need, TESTS_BY_PACKAGE_OUTPUT, TESTS_IGNORED_OUTPUT, TESTS_LISTED_OUTPUT, TESTS_RUN_OUTPUT,
    TestCounts, parse_package_counts,
};

pub(super) fn counts(need: Option<&Need>) -> Option<TestCounts> {
    let need = need?;
    let packages = parse_package_counts(need.outputs.get(TESTS_BY_PACKAGE_OUTPUT)?)?;
    let run = number(need.outputs.get(TESTS_RUN_OUTPUT)?)?;
    let listed = number(need.outputs.get(TESTS_LISTED_OUTPUT)?)?;
    let summed_run = packages
        .values()
        .try_fold(0u64, |total, package| total.checked_add(package.run))?;
    let summed_listed = packages
        .values()
        .try_fold(0u64, |total, package| total.checked_add(package.listed))?;
    if run != summed_run || listed != summed_listed {
        return None;
    }
    let ignored = match need.outputs.get(TESTS_IGNORED_OUTPUT).map(String::as_str) {
        None | Some("") => None,
        Some(text) => Some(number(text)?),
    };
    Some(TestCounts {
        run,
        listed,
        ignored,
        packages,
    })
}

pub(super) fn number(text: &str) -> Option<u64> {
    if text.is_empty()
        || (text.len() > 1 && text.starts_with('0'))
        || !text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    text.parse().ok()
}
