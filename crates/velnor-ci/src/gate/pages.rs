// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::de::DeserializeOwned;

use crate::error::CiError;
use crate::github::{ApiPage, GitHub, parse};

pub(super) fn all_pages<T: DeserializeOwned>(
    github: &dyn GitHub,
    what: &str,
    path: impl Fn(u32) -> String,
) -> Result<Vec<T>, CiError> {
    pages(github, what, path, EmptyPage::Reject)
}

#[derive(Clone, Copy)]
pub(super) enum EmptyPage {
    Reject,
    Discard,
}

pub(super) fn pages<T: DeserializeOwned>(
    github: &dyn GitHub,
    what: &str,
    path: impl Fn(u32) -> String,
    empty_page: EmptyPage,
) -> Result<Vec<T>, CiError> {
    let mut all = Vec::new();
    let mut expected = None;
    for page in 1.. {
        let ApiPage { total_count, items } = parse(&github.get(&path(page))?, what)?;
        if expected.is_some_and(|count| count != total_count) {
            return Err(CiError::new(format!(
                "GitHub {what} listing changed during pagination; retry the release gate"
            )));
        }
        expected = Some(total_count);
        let empty = items.is_empty();
        all.extend(items);
        if all.len() == total_count {
            break;
        }
        if empty && matches!(empty_page, EmptyPage::Discard) {
            return Ok(Vec::new());
        }
        if empty || all.len() > total_count {
            return Err(CiError::new(format!(
                "GitHub lists {total_count} {what} and returned {}",
                all.len()
            )));
        }
    }
    Ok(all)
}
