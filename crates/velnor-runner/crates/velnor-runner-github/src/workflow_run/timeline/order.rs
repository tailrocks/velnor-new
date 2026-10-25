//! Strict ordering and proof checks for base-ref changes.

use super::super::WorkflowRunError;
use super::dto::{BaseRefChangedEvent, PullRequestBaseHistory};

/// Order events and require one complete chain ending at the current base.
///
/// # Errors
///
/// Gives [`WorkflowRunError::Malformed`] when event order or the current base is incorrect.
pub fn ordered_base_changes(
    history: &PullRequestBaseHistory,
) -> Result<Vec<BaseRefChangedEvent>, WorkflowRunError> {
    let mut ordered = history.events.clone();
    ordered.sort_by_key(|event| timestamp_key(&event.created_at));
    let mut keys = Vec::with_capacity(ordered.len());
    let mut ids = std::collections::HashSet::new();
    for event in &ordered {
        let key = timestamp_key(&event.created_at).ok_or(WorkflowRunError::Malformed)?;
        keys.push(key);
        if event.id.is_empty()
            || !ids.insert(&event.id)
            || event.previous_ref_name.is_empty()
            || event.current_ref_name.is_empty()
            || event.previous_ref_name == event.current_ref_name
        {
            return Err(WorkflowRunError::Malformed);
        }
    }
    if keys.windows(2).any(|pair| pair[0] == pair[1])
        || ordered
            .windows(2)
            .any(|pair| pair[0].current_ref_name != pair[1].previous_ref_name)
        || ordered
            .last()
            .is_some_and(|event| event.current_ref_name != history.base_ref)
    {
        return Err(WorkflowRunError::Malformed);
    }
    Ok(ordered)
}

/// Find the one new final event between two exact pull request snapshots.
///
/// # Errors
///
/// Gives [`WorkflowRunError::Malformed`] when snapshots differ outside the base change.
pub fn new_retarget_event<'a>(
    before: &PullRequestBaseHistory,
    after: &'a PullRequestBaseHistory,
    previous_ref: &str,
    current_ref: &str,
) -> Result<&'a BaseRefChangedEvent, WorkflowRunError> {
    if !same_head_snapshot(before, after)
        || before.base_ref != previous_ref
        || after.base_ref != current_ref
        || after.events.len() != before.events.len().saturating_add(1)
    {
        return Err(WorkflowRunError::Malformed);
    }
    for old in &before.events {
        if !after.events.iter().any(|new| new == old) {
            return Err(WorkflowRunError::Malformed);
        }
    }
    let mut added = after
        .events
        .iter()
        .filter(|event| !before.events.iter().any(|old| old.id == event.id));
    let event = added.next().ok_or(WorkflowRunError::Malformed)?;
    if added.next().is_some() {
        return Err(WorkflowRunError::Malformed);
    }
    retarget_event_after_acknowledgement(after, &event.id, previous_ref, current_ref)
}

/// Find one acknowledged base change and reject any later base change.
///
/// # Errors
///
/// Gives [`WorkflowRunError::Malformed`] when the event is absent, different, or no longer final.
pub fn retarget_event_after_acknowledgement<'a>(
    history: &'a PullRequestBaseHistory,
    event_id: &str,
    previous_ref: &str,
    current_ref: &str,
) -> Result<&'a BaseRefChangedEvent, WorkflowRunError> {
    let ordered = ordered_base_changes(history)?;
    let Some(event) = ordered.last() else {
        return Err(WorkflowRunError::Malformed);
    };
    if event.id != event_id
        || event.previous_ref_name != previous_ref
        || event.current_ref_name != current_ref
        || history.base_ref != current_ref
    {
        return Err(WorkflowRunError::Malformed);
    }
    history
        .events
        .iter()
        .find(|candidate| candidate.id == event_id)
        .ok_or(WorkflowRunError::Malformed)
}

fn same_head_snapshot(left: &PullRequestBaseHistory, right: &PullRequestBaseHistory) -> bool {
    left.number == right.number
        && left.repository_id == right.repository_id
        && left.head_repository_id == right.head_repository_id
        && left.head_ref == right.head_ref
        && left.head_sha == right.head_sha
        && left.base_repository_id == right.base_repository_id
}

fn timestamp_key(value: &str) -> Option<i128> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let year = number(&bytes[0..4])?;
    let month = number(&bytes[5..7])?;
    let day = number(&bytes[8..10])?;
    let hour = number(&bytes[11..13])?;
    let minute = number(&bytes[14..16])?;
    let second = number(&bytes[17..19])?;
    if year == 0
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let mut index = 19;
    let mut nanos = 0_i128;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        let digits = index.saturating_sub(start);
        if digits == 0 || digits > 9 {
            return None;
        }
        let fraction = i128::from(number(&bytes[start..index])?);
        nanos = fraction * 10_i128.pow(u32::try_from(9 - digits).ok()?);
    }
    let offset_seconds = match bytes.get(index).copied()? {
        b'Z' if index + 1 == bytes.len() => 0_i128,
        sign @ (b'+' | b'-') if index + 6 == bytes.len() && bytes.get(index + 3) == Some(&b':') => {
            let offset_hour = number(&bytes[index + 1..index + 3])?;
            let offset_minute = number(&bytes[index + 4..index + 6])?;
            if offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            let value = i128::from(offset_hour * 3600 + offset_minute * 60);
            if sign == b'+' { value } else { -value }
        }
        _ => return None,
    };
    let days = days_from_civil(year, month, day)?;
    Some(
        (i128::from(days) * 86_400 + i128::from(hour * 3600 + minute * 60 + second)
            - offset_seconds)
            * 1_000_000_000
            + nanos,
    )
}

fn number(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    bytes.iter().try_fold(0_u32, |value, byte| {
        value.checked_mul(10)?.checked_add(u32::from(*byte - b'0'))
    })
}

fn days_from_civil(year: u32, month: u32, day: u32) -> Option<i64> {
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if day == 0 || day > days_in_month {
        return None;
    }
    let adjusted_year = i64::from(year) - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}
