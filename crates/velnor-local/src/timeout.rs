// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const KEY: &str = "timeout-minutes:";
const UNIT: u64 = 1000;

/// A factor for `timeout-minutes`, held in thousandths so that a decimal factor multiplies exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeoutScale(u64);

impl Default for TimeoutScale {
    fn default() -> Self {
        Self(UNIT)
    }
}

/// Parses a factor of at least 1 with at most three decimals, such as `3` or `1.5`.
pub fn parse_timeout_scale(text: &str) -> Result<TimeoutScale, String> {
    let refused = || {
        format!(
            "--timeout-scale {text:?} must be a number of at least 1 with at most three decimals, such as 3 or 1.5"
        )
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if !digits(whole) || (text.contains('.') && !digits(fraction)) || fraction.len() > 3 {
        return Err(refused());
    }
    let whole: u64 = whole.parse().map_err(|_| refused())?;
    let fraction: u64 = format!("{fraction:0<3}").parse().map_err(|_| refused())?;
    let thousandths = whole
        .checked_mul(UNIT)
        .and_then(|scaled| scaled.checked_add(fraction))
        .ok_or_else(refused)?;
    if thousandths < UNIT {
        return Err(refused());
    }
    Ok(TimeoutScale(thousandths))
}

/// Multiplies every numeric `timeout-minutes` value of a workflow by `by`, rounding up to whole
/// minutes. A value that is an expression, quoted or followed by something other than a comment is
/// left alone, and so is a value the factor leaves unchanged. Returns the rewritten workflow and the
/// number of values it changed.
#[must_use]
pub fn scale(workflow: &str, by: TimeoutScale) -> (String, usize) {
    let mut count = 0;
    let mut rewritten = String::with_capacity(workflow.len());
    for line in workflow.split_inclusive('\n') {
        match scale_line(line, by) {
            Some(line) => {
                count += 1;
                rewritten.push_str(&line);
            }
            None => rewritten.push_str(line),
        }
    }
    (rewritten, count)
}

fn scale_line(line: &str, by: TimeoutScale) -> Option<String> {
    let at = line.find(KEY)?;
    if !matches!(line[..at].trim(), "" | "-") {
        return None;
    }
    let after = at + KEY.len();
    let start = after + (line[after..].len() - line[after..].trim_start().len());
    let value = &line[start..];
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let tail = &value[digits..];
    let comment_or_end = tail.trim_start();
    let plain = tail.is_empty()
        || (tail.starts_with(char::is_whitespace)
            && (comment_or_end.is_empty() || comment_or_end.starts_with('#')));
    if !plain {
        return None;
    }
    let minutes: u64 = value[..digits].parse().ok()?;
    let scaled = minutes.saturating_mul(by.0).div_ceil(UNIT);
    (scaled != minutes).then(|| format!("{}{scaled}{tail}", &line[..start]))
}
