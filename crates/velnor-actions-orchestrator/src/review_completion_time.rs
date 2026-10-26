// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Timestamp {
    parts: [u16; 6],
    nanos: u32,
}

pub(super) fn parse(value: &str) -> Result<Timestamp, OrchestratorError> {
    let b = value.as_bytes();
    if !value.is_ascii()
        || b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b.last() != Some(&b'Z')
    {
        return Err(internal("review_provider_time"));
    }
    let mut parts = [0_u16; 6];
    for (slot, (start, end)) in
        parts
            .iter_mut()
            .zip([(0, 4), (5, 7), (8, 10), (11, 13), (14, 16), (17, 19)])
    {
        let digits = &value[start..end];
        if !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(internal("review_provider_time"));
        }
        *slot = digits
            .parse()
            .map_err(|_| internal("review_provider_time"))?;
    }
    let [year, month, day, hour, minute, second] = parts;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => 0,
    };
    if year == 0 || day == 0 || day > days || hour > 23 || minute > 59 || second > 59 {
        return Err(internal("review_provider_time"));
    }
    let mut nanos = 0;
    if b.len() != 20 {
        let fraction = &value[19..value.len() - 1];
        if !fraction.starts_with('.')
            || !(2..=10).contains(&fraction.len())
            || !fraction[1..].bytes().all(|b| b.is_ascii_digit())
        {
            return Err(internal("review_provider_time"));
        }
        nanos = fraction[1..]
            .parse::<u32>()
            .map_err(|_| internal("review_provider_time"))?;
        for _ in fraction.len() - 1..9 {
            nanos *= 10;
        }
    }
    Ok(Timestamp { parts, nanos })
}

pub(super) fn terminal(body: &str, role: &str) -> Result<Timestamp, OrchestratorError> {
    let prefix = match role {
        "code" => "| 📝 **Code Review** | ✅ **Completed** <relative-time datetime=\"",
        "security" => "| 🔒 **Security Review** | ✅ **Completed** <relative-time datetime=\"",
        _ => return Err(internal("review_provider_role")),
    };
    let rows = body
        .lines()
        .filter_map(|line| line.strip_prefix(prefix))
        .collect::<Vec<_>>();
    if rows.len() != 1 {
        return Err(internal("review_provider_terminal_time"));
    }
    let (stamp, rest) = rows[0]
        .split_once('"')
        .ok_or_else(|| internal("review_provider_terminal_time"))?;
    if !rest.starts_with('>') || !rest.contains("</relative-time>") {
        return Err(internal("review_provider_terminal_time"));
    }
    parse(stamp)
}
