pub(super) fn valid_timestamp(value: &str) -> bool {
    let Some((date, clock)) = value.split_once('T') else {
        return false;
    };
    let Some(clock) = clock.strip_suffix('Z') else {
        return false;
    };
    let Some((hours, minutes, seconds, fraction)) = timestamp_parts(clock) else {
        return false;
    };
    let Some((year, month, day)) = date_parts(date) else {
        return false;
    };
    let month_days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return false,
    };
    year > 0
        && day > 0
        && day <= month_days
        && hours < 24
        && minutes < 60
        && seconds < 60
        && fraction
}

fn date_parts(date: &str) -> Option<(u32, u32, u32)> {
    if date.len() != 10
        || date.as_bytes().get(4) != Some(&b'-')
        || date.as_bytes().get(7) != Some(&b'-')
    {
        return None;
    }
    if !date
        .bytes()
        .enumerate()
        .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return None;
    }
    Some((
        date[0..4].parse().ok()?,
        date[5..7].parse().ok()?,
        date[8..10].parse().ok()?,
    ))
}

fn timestamp_parts(clock: &str) -> Option<(u32, u32, u32, bool)> {
    let (time, fraction) = clock
        .split_once('.')
        .map_or((clock, None), |(time, fraction)| (time, Some(fraction)));
    if time.len() != 8
        || time.as_bytes().get(2) != Some(&b':')
        || time.as_bytes().get(5) != Some(&b':')
    {
        return None;
    }
    if !time
        .bytes()
        .enumerate()
        .all(|(index, byte)| matches!(index, 2 | 5) || byte.is_ascii_digit())
    {
        return None;
    }
    let fraction_valid = fraction.is_none_or(|value| {
        (1..=9).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
    });
    Some((
        time[0..2].parse().ok()?,
        time[3..5].parse().ok()?,
        time[6..8].parse().ok()?,
        fraction_valid,
    ))
}

fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}
