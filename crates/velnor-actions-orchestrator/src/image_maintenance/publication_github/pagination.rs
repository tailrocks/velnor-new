// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::de::DeserializeOwned;
use serde_json::Value;
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::get_json;

const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 20;
const MAX_ITEMS: usize = PAGE_SIZE * MAX_PAGES;
const MAX_BYTES: usize = 16 * 1024 * 1024;

pub(super) struct Collection<'a, T> {
    pub(super) route: &'a str,
    pub(super) name: &'a str,
    pub(super) max_items: usize,
    pub(super) error: &'static str,
    pub(super) identity_error: &'static str,
    pub(super) id: fn(&T) -> Option<u64>,
}

pub(super) fn get_complete<T: DeserializeOwned>(
    collection: &Collection<'_, T>,
    token: &str,
    deadline: CheckDeadline,
) -> Result<Vec<T>, OrchestratorError> {
    let Collection {
        route,
        name,
        max_items,
        error,
        identity_error,
        id,
    } = *collection;
    let mut total_count = None;
    let mut items = Vec::new();
    let mut ids = BTreeSet::new();
    let mut bytes = 0_usize;
    for page in 1..=MAX_PAGES {
        let separator = if route.contains('?') { '&' } else { '?' };
        let page_route = format!("{route}{separator}per_page={PAGE_SIZE}&page={page}");
        let response: Value = get_json(&page_route, token, deadline)?;
        bytes = bytes
            .checked_add(
                serde_json::to_vec(&response)
                    .map_err(|_| internal(error))?
                    .len(),
            )
            .filter(|size| *size <= MAX_BYTES)
            .ok_or_else(|| internal(error))?;
        let object = response.as_object().ok_or_else(|| internal(error))?;
        let count = object
            .get("total_count")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value <= max_items && *value <= MAX_ITEMS)
            .ok_or_else(|| internal(error))?;
        if total_count.is_some_and(|previous| previous != count) {
            return Err(internal(error));
        }
        total_count = Some(count);
        let page_items = object
            .get(name)
            .and_then(Value::as_array)
            .ok_or_else(|| internal(error))?;
        let expected = count.saturating_sub(items.len()).min(PAGE_SIZE);
        if page_items.len() != expected {
            return Err(internal(error));
        }
        for value in page_items {
            let item: T = serde_json::from_value(value.clone()).map_err(|_| internal(error))?;
            let item_id = id(&item)
                .filter(|item_id| *item_id != 0)
                .ok_or_else(|| internal(identity_error))?;
            if !ids.insert(item_id) {
                return Err(internal(identity_error));
            }
            items.push(item);
        }
        if items.len() == count {
            return Ok(items);
        }
    }
    Err(internal(error))
}
