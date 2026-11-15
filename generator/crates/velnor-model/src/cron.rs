// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use crate::ids::InvalidId;

const FIELD_RANGES: [(u32, u32); 5] = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 6)];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CronExpr(String);

impl CronExpr {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidId> {
        let value = value.into();
        let fields: Vec<&str> = value.split(' ').collect();
        let valid = fields.len() == FIELD_RANGES.len()
            && fields
                .iter()
                .zip(FIELD_RANGES)
                .all(|(field, range)| field_valid(field, range));
        if valid {
            Ok(Self(value))
        } else {
            Err(InvalidId {
                kind: "CronExpr",
                value,
            })
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CronExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn field_valid(field: &str, range: (u32, u32)) -> bool {
    field.split(',').all(|item| item_valid(item, range))
}

fn item_valid(item: &str, (low, high): (u32, u32)) -> bool {
    let (base, step) = match item.split_once('/') {
        Some((base, step)) => (base, Some(step)),
        None => (item, None),
    };
    let within = |text: &str| number(text).filter(|n| (low..=high).contains(n));
    let span = if base == "*" {
        true
    } else if let Some((from, to)) = base.split_once('-') {
        matches!((within(from), within(to)), (Some(from), Some(to)) if from <= to)
    } else {
        within(base).is_some()
    };
    span && step.is_none_or(|step| number(step).is_some_and(|n| (1..=high).contains(&n)))
}

fn number(text: &str) -> Option<u32> {
    if text.is_empty() || text.len() > 2 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}
