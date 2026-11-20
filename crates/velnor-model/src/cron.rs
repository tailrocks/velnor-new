// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use crate::ids::InvalidId;

const FIELD_RANGES: &[(u32, u32)] = &[(0, 59), (0, 23), (1, 31), (1, 12), (0, 6)];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CronExpr(String);

impl CronExpr {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidId> {
        let value = value.into();
        let fields: Vec<&str> = value.split(' ').collect();
        let valid = fields.len() == FIELD_RANGES.len()
            && fields
                .iter()
                .zip(FIELD_RANGES.iter().copied())
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
    pub fn shortest_gap_minutes(&self) -> Option<u32> {
        let fields: Vec<&str> = self.0.split(' ').collect();
        let minutes = expand(fields[0], FIELD_RANGES[0]);
        let hours = expand(fields[1], FIELD_RANGES[1]);
        let mut times: Vec<u32> = hours
            .iter()
            .flat_map(|hour| minutes.iter().map(move |minute| hour * 60 + minute))
            .collect();
        times.sort_unstable();
        let (first, last) = times.first().copied().zip(times.last().copied())?;
        let days = matching_days(&fields);
        if days.is_empty() {
            return None;
        }
        let same_day = times.windows(2).map(|pair| pair[1] - pair[0]).min();
        let between_days = days
            .windows(2)
            .map(|pair| (pair[1] - pair[0]) * MINUTES_PER_DAY + first - last)
            .min();
        Some(
            same_day
                .into_iter()
                .chain(between_days)
                .min()
                .unwrap_or(u32::MAX),
        )
    }
}

const MINUTES_PER_DAY: u32 = 24 * 60;
const FIRST_YEAR: u32 = 2024;
const YEARS_SCANNED: u32 = 8;
const FIRST_DAY_OF_WEEK: u32 = 1;

fn matching_days(fields: &[&str]) -> Vec<u32> {
    let months = expand(fields[3], FIELD_RANGES[3]);
    let month_days = expand(fields[2], FIELD_RANGES[2]);
    let weekdays = expand(fields[4], FIELD_RANGES[4]);
    let either = !fields[2].starts_with('*') && !fields[4].starts_with('*');
    let mut matching = Vec::new();
    let mut index = 0;
    for year in FIRST_YEAR..FIRST_YEAR + YEARS_SCANNED {
        for month in 1..=12 {
            for day in 1..=days_in_month(year, month) {
                let on_day = month_days.contains(&day);
                let on_weekday = weekdays.contains(&((FIRST_DAY_OF_WEEK + index) % 7));
                let day_matches = if either {
                    on_day || on_weekday
                } else {
                    on_day && on_weekday
                };
                if months.contains(&month) && day_matches {
                    matching.push(index);
                }
                index += 1;
            }
        }
    }
    matching
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

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
