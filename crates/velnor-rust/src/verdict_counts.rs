// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Need, RootSlug, TESTS_BY_PACKAGE_OUTPUT, TESTS_IGNORED_OUTPUT, TESTS_LISTED_OUTPUT,
    TESTS_RUN_OUTPUT, TestCounts, parse_package_counts,
};

pub(super) fn counts(need: Option<&Need>, slug: &RootSlug) -> Option<TestCounts> {
    let need = need?;
    let named = |name: &str| {
        need.outputs
            .get(&format!("{name}_{slug}"))
            .or_else(|| need.outputs.get(name))
    };
    let packages = parse_package_counts(named(TESTS_BY_PACKAGE_OUTPUT)?)?;
    let run = number(named(TESTS_RUN_OUTPUT)?)?;
    let listed = number(named(TESTS_LISTED_OUTPUT)?)?;
    let summed_run = packages
        .values()
        .try_fold(0u64, |total, package| total.checked_add(package.run))?;
    let summed_listed = packages
        .values()
        .try_fold(0u64, |total, package| total.checked_add(package.listed))?;
    if run != summed_run || listed != summed_listed {
        return None;
    }
    let ignored = match named(TESTS_IGNORED_OUTPUT).map(String::as_str) {
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
