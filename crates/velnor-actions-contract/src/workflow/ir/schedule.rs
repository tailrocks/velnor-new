// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleTrigger {
    pub cron: Vec<String>,
}

impl ScheduleTrigger {
    #[must_use]
    pub fn new(cron: Vec<String>) -> Self {
        Self { cron }
    }

    #[must_use]
    pub fn single(expr: impl Into<String>) -> Self {
        Self {
            cron: vec![expr.into()],
        }
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        if self.cron.is_empty() {
            return Err(ContractError::identity("schedule.cron", "empty_cron"));
        }
        let bounds: [(u32, u32); 5] = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)];
        for entry in &self.cron {
            let fields: Vec<&str> = entry.split_whitespace().collect();
            let valid = fields.len() == 5
                && fields
                    .into_iter()
                    .zip(bounds)
                    .all(|(field, (min, max))| is_valid_cron_field(field, min, max));
            if !valid {
                return Err(ContractError::identity(
                    "schedule.cron",
                    format!("bad_cron:{entry}"),
                ));
            }
        }
        Ok(())
    }
}

fn is_valid_cron_field(field: &str, min: u32, max: u32) -> bool {
    !field.is_empty()
        && field
            .split(',')
            .all(|part| is_valid_cron_part(part, min, max))
}

fn is_valid_cron_part(part: &str, min: u32, max: u32) -> bool {
    if part.is_empty() {
        return false;
    }
    let (range_part, step_ok) = match part.split_once('/') {
        Some((base, step)) => (
            base,
            parse_cron_num(step).is_some_and(|s| s >= 1 && s <= max.max(1)),
        ),
        None => (part, true),
    };
    if !step_ok || range_part.is_empty() {
        return false;
    }
    if range_part == "*" {
        return true;
    }
    if let Some((start_str, end_str)) = range_part.split_once('-') {
        let Some(start) = parse_cron_num(start_str) else {
            return false;
        };
        let Some(end) = parse_cron_num(end_str) else {
            return false;
        };
        start >= min && end <= max && start <= end
    } else {
        parse_cron_num(range_part).is_some_and(|n| n >= min && n <= max)
    }
}

fn parse_cron_num(text: &str) -> Option<u32> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse::<u32>().ok())
        .flatten()
}
