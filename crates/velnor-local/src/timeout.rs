// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ops::Range;

use serde_saphyr::granit_parser::{Event, Parser, ScalarStyle, Span};

const KEY: &str = "timeout-minutes";
const MERGE: &str = "<<";
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

/// Multiplies every numeric `timeout-minutes` value at job or step level of a workflow by `by`,
/// rounding up to whole minutes, editing only the value so comments and formatting survive. A value
/// that is an expression, quoted or not a plain number is left alone. A workflow whose timeout is an
/// alias or an anchor, or a product that does not fit `u64`, is refused. Returns the rewritten
/// workflow and the number of values it changed.
pub fn scale(workflow: &str, by: TimeoutScale) -> Result<(String, usize), String> {
    if by == TimeoutScale::default() {
        return Ok((workflow.to_owned(), 0));
    }
    let mut edits = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    for item in Parser::new_from_str(workflow) {
        let (event, span) = item.map_err(|error| format!("not valid YAML: {error}"))?;
        match event {
            Event::Scalar(value, style, anchor, tag) => {
                if matches!(stack.last(), Some(Frame::Map { value: false, .. })) {
                    set_key(&mut stack, &value);
                    continue;
                }
                if at(&stack, KEY) {
                    if anchor != 0 {
                        return Err(refusal("an anchor", &span));
                    }
                    if style == ScalarStyle::Plain
                        && tag.is_none()
                        && let Some(edit) = rescale(workflow, &value, &span, by)?
                    {
                        edits.push(edit);
                    }
                }
                finish(&mut stack);
            }
            Event::Alias(_) | Event::UnresolvedAlias(_) => {
                if at(&stack, KEY) || at(&stack, MERGE) {
                    return Err(refusal("an alias", &span));
                }
                finish(&mut stack);
            }
            Event::MappingStart(..) => stack.push(Frame::Map {
                key: None,
                value: false,
            }),
            Event::SequenceStart(..) => stack.push(Frame::Seq),
            Event::MappingEnd | Event::SequenceEnd => {
                stack.pop();
                finish(&mut stack);
            }
            _ => {}
        }
    }
    let count = edits.len();
    let mut rewritten = workflow.to_owned();
    for (range, text) in edits.into_iter().rev() {
        rewritten.replace_range(range, &text);
    }
    Ok((rewritten, count))
}

enum Frame {
    Map { key: Option<String>, value: bool },
    Seq,
}

fn set_key(stack: &mut [Frame], name: &str) {
    if let Some(Frame::Map { key, value }) = stack.last_mut() {
        *key = Some(name.to_owned());
        *value = true;
    }
}

fn finish(stack: &mut [Frame]) {
    if let Some(Frame::Map { key, value }) = stack.last_mut() {
        if *value {
            *key = None;
        }
        *value = !*value;
    }
}

fn at(stack: &[Frame], name: &str) -> bool {
    let path: Vec<Option<&str>> = stack
        .iter()
        .map(|frame| match frame {
            Frame::Map { key, .. } => Some(key.as_deref().unwrap_or("")),
            Frame::Seq => None,
        })
        .collect();
    match path.as_slice() {
        [Some("jobs"), Some(_), Some(last)]
        | [Some("jobs"), Some(_), Some("steps"), None, Some(last)] => *last == name,
        _ => false,
    }
}

fn refusal(form: &str, span: &Span) -> String {
    format!(
        "--timeout-scale cannot rewrite a timeout-minutes given as {form} (line {}); write the number in place",
        span.start.line()
    )
}

fn rescale(
    workflow: &str,
    value: &str,
    span: &Span,
    by: TimeoutScale,
) -> Result<Option<(Range<usize>, String)>, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    let line = span.start.line();
    let range = span
        .byte_range()
        .filter(|range| workflow.get(range.clone()) == Some(value))
        .ok_or_else(|| {
            format!("--timeout-scale cannot locate timeout-minutes {value} (line {line})")
        })?;
    let overflow = || {
        format!(
            "--timeout-scale: timeout-minutes {value} times the factor does not fit in 64 bits (line {line})"
        )
    };
    let minutes: u64 = value.parse().map_err(|_| overflow())?;
    let scaled = (u128::from(minutes) * u128::from(by.0)).div_ceil(u128::from(UNIT));
    let scaled = u64::try_from(scaled).map_err(|_| overflow())?;
    Ok((scaled != minutes).then(|| (range, scaled.to_string())))
}
