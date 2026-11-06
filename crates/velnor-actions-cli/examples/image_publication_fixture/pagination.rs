// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

pub(super) fn page_number(route: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let query = route
        .split_once('?')
        .map(|(_, query)| query)
        .ok_or("missing query")?;
    let fields = query.split('&').collect::<Vec<_>>();
    let pages = fields
        .iter()
        .filter_map(|field| field.strip_prefix("page="))
        .collect::<Vec<_>>();
    let page = match pages.as_slice() {
        [] => 1,
        [page] => page.parse::<usize>()?,
        _ => return Err("duplicate page parameter".into()),
    };
    let per_page = fields
        .iter()
        .filter_map(|field| field.strip_prefix("per_page="))
        .collect::<Vec<_>>();
    let nonces = fields
        .iter()
        .filter_map(|field| field.strip_prefix("_velnor_read_nonce="))
        .collect::<Vec<_>>();
    if page == 0 || per_page != ["100"] || nonces.len() != 1 || nonces[0].is_empty() {
        return Err("invalid paginated API query".into());
    }
    Ok(page)
}

pub(super) fn page(
    route: &str,
    state: &Value,
    pages_key: &str,
    first_page: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let page = page_number(route)?;
    match state[pages_key].as_array() {
        Some(pages) => pages
            .get(page - 1)
            .cloned()
            .ok_or_else(|| format!("unexpected {pages_key} page {page}").into()),
        None if page == 1 => Ok(first_page),
        None => Err(format!("unexpected {pages_key} page {page}").into()),
    }
}
