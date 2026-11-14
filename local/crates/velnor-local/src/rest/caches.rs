// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::store::Record;

#[must_use]
pub fn rfc3339(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or(0) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let rest = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

pub fn cache_list(records: &[Record], params: &BTreeMap<String, String>) -> Vec<Value> {
    let mut matching: Vec<&Record> = records
        .iter()
        .filter(|record| {
            params
                .get("ref")
                .is_none_or(|git_ref| *git_ref == record.scope)
        })
        .filter(|record| {
            params
                .get("key")
                .is_none_or(|key| record.key.starts_with(key))
        })
        .collect();
    let sort = params.get("sort").map(String::as_str);
    matching.sort_by_key(|record| {
        (
            match sort {
                Some("created_at") => record.created,
                Some("size_in_bytes") => record.size_bytes,
                _ => record.last_access,
            },
            record.id,
        )
    });
    if params.get("direction").map(String::as_str) != Some("asc") {
        matching.reverse();
    }
    matching
        .into_iter()
        .map(|record| {
            json!({
                "id": record.id,
                "ref": record.scope,
                "key": record.key,
                "version": record.version,
                "last_accessed_at": rfc3339(record.last_access),
                "created_at": rfc3339(record.created),
                "size_in_bytes": record.size_bytes,
            })
        })
        .collect()
}
