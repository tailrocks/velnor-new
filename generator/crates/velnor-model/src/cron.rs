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

    #[must_use]
    pub fn shortest_gap_minutes(&self) -> u32 {
        let fields: Vec<&str> = self.0.split(' ').collect();
        let minutes = expand(fields[0], FIELD_RANGES[0]);
        let hours = expand(fields[1], FIELD_RANGES[1]);
        let mut times: Vec<u32> = hours
            .iter()
            .flat_map(|hour| minutes.iter().map(move |minute| hour * 60 + minute))
            .collect();
        times.sort_unstable();
        let wrap = times
            .first()
            .zip(times.last())
            .map_or(MINUTES_PER_DAY, |(first, last)| {
                first + MINUTES_PER_DAY - last
            });
        times
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .fold(wrap, u32::min)
    }
}

const MINUTES_PER_DAY: u32 = 24 * 60;

fn expand(field: &str, (low, high): (u32, u32)) -> Vec<u32> {
    let mut values: Vec<u32> = field
        .split(',')
        .flat_map(|item| {
            let (base, step) = match item.split_once('/') {
                Some((base, step)) => (base, number(step).unwrap_or(1)),
                None => (item, 1),
            };
            let (from, to) = if base == "*" {
                (low, high)
            } else if let Some((from, to)) = base.split_once('-') {
                (number(from).unwrap_or(low), number(to).unwrap_or(high))
            } else {
                let start = number(base).unwrap_or(low);
                (start, if item.contains('/') { high } else { start })
            };
            (from..=to).step_by(step.max(1) as usize)
        })
        .collect();
    values.sort_unstable();
    values.dedup();
    values
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
