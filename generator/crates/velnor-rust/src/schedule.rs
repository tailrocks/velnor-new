// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CronExpr, FullRun, RepoFacts};

use crate::sha256::sha256_hex;

const MINUTES_USED: u32 = 58;
const HOURS: u32 = 24;
const WEEKDAYS: u32 = 7;
const MINUTES_PER_DAY: u32 = 1440;
const SEED_DIGITS: usize = 8;

pub(crate) fn schedule(facts: &RepoFacts) -> Option<CronExpr> {
    let seed =
        u32::from_str_radix(&sha256_hex(facts.repository.as_bytes())[..SEED_DIGITS], 16).ok()?;
    let minute = 1 + seed % MINUTES_USED;
    let hour = (seed / MINUTES_USED) % HOURS;
    let weekday = (seed / MINUTES_PER_DAY) % WEEKDAYS;
    let text = match &facts.config.full_run {
        FullRun::Off => return None,
        FullRun::Hourly => format!("{minute} * * * *"),
        FullRun::Nightly => format!("{minute} {hour} * * *"),
        FullRun::Weekly => format!("{minute} {hour} * * {weekday}"),
        FullRun::Cron(expression) => return Some(expression.clone()),
    };
    CronExpr::new(text).ok()
}
