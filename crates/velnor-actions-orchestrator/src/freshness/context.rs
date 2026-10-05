//! Shared freshness state, row output, and date parsing.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use super::rust_source::extract_public_string_const;

pub(super) const RUST_SOURCE_CAP: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CivilDate {
    pub(super) year: i64,
    pub(super) month: u32,
    pub(super) day: u32,
}

impl fmt::Display for CivilDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

#[derive(Debug)]
pub(super) struct FreshnessContext {
    pub(super) root: PathBuf,
    pub(super) inv_path: PathBuf,
    pub(super) check_upstream: bool,
    pub(super) with_advisories: bool,
    pub(super) failures: Vec<String>,
    pub(super) now_micros: i64,
    pub(super) inv: Value,
    pub(super) interval: i64,
    pub(super) max_days: i64,
    pub(super) top_checked: Option<Value>,
    pub(super) policy: Option<Value>,
    pub(super) tools: Vec<Value>,
    pub(super) action_pinned: BTreeMap<String, Value>,
    pub(super) tool_pinned: BTreeMap<String, Value>,
    pub(super) runner: Value,
    pub(super) supported: Vec<Value>,
    pub(super) locked: Option<Vec<Value>>,
    pub(super) member_names: BTreeSet<String>,
    pub(super) holds: Vec<Value>,
    pub(super) hold_keys: BTreeSet<String>,
    pub(super) early_error: Option<String>,
    output: Vec<String>,
}

impl FreshnessContext {
    pub(super) fn new(root: &Path, check_upstream: bool, with_advisories: bool) -> Self {
        let now_micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
            });
        Self {
            root: root.to_path_buf(),
            inv_path: root.join(".velnor/freshness-inventory.json"),
            check_upstream,
            with_advisories,
            failures: Vec::new(),
            now_micros,
            inv: Value::Object(Map::new()),
            interval: 24,
            max_days: 14,
            top_checked: None,
            policy: None,
            tools: Vec::new(),
            action_pinned: BTreeMap::new(),
            tool_pinned: BTreeMap::new(),
            runner: Value::Object(Map::new()),
            supported: Vec::new(),
            locked: None,
            member_names: BTreeSet::new(),
            holds: Vec::new(),
            hold_keys: BTreeSet::new(),
            early_error: None,
            output: Vec::new(),
        }
    }

    pub(super) fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub(super) fn row(&mut self, check: &str, subject: &str, status: &str, detail: &str) {
        let mut fields = Map::new();
        fields.insert("check".to_owned(), json!(check));
        fields.insert("subject".to_owned(), json!(subject));
        fields.insert("status".to_owned(), json!(status));
        fields.insert("detail".to_owned(), json!(detail));
        let payload =
            serde_json::to_string(&Value::Object(fields)).unwrap_or_else(|_| "{}".to_owned());
        self.output.push(format!("row: {payload}"));
    }

    pub(super) fn fail_row(&mut self, check: &str, subject: &str, detail: &str) {
        self.row(check, subject, "fail", detail);
        self.failures.push(format!("{check} {subject}: {detail}"));
    }

    pub(super) fn pass_row(&mut self, check: &str, subject: &str, detail: &str) {
        self.row(check, subject, "pass", detail);
        let line = format!("ok: {check} {subject} {detail}")
            .trim_end()
            .to_owned();
        self.output.push(line);
    }

    pub(super) fn info_row(&mut self, check: &str, subject: &str, detail: &str) {
        self.row(check, subject, "info", detail);
    }

    pub(super) fn rust_const(&mut self, relative: &str, name: &str) -> Option<String> {
        let path = self.path(relative);
        let subject = format!("{relative}::{name}");
        let source = match std::fs::read(&path) {
            Ok(source) => source,
            Err(error) => {
                self.fail_row("local-pin", &subject, &format!("unreadable ({error})"));
                return None;
            }
        };
        if source.len() > RUST_SOURCE_CAP {
            self.fail_row(
                "local-pin",
                &subject,
                &format!("source exceeds {RUST_SOURCE_CAP} bytes"),
            );
            return None;
        }
        let source = match std::str::from_utf8(&source) {
            Ok(source) => source,
            Err(error) => {
                self.fail_row(
                    "local-pin",
                    &subject,
                    &format!("unsupported Rust source ({error})"),
                );
                return None;
            }
        };
        match extract_public_string_const(source, name) {
            Ok(value) => Some(value),
            Err(error) => {
                self.fail_row(
                    "local-pin",
                    &subject,
                    &format!("unsupported Rust source ({error})"),
                );
                None
            }
        }
    }

    pub(super) fn finish(self) -> io::Result<i32> {
        if let Some(error) = self.early_error {
            writeln!(io::stderr().lock(), "check-freshness: {error}")?;
            return Ok(1);
        }
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        for line in self.output {
            writeln!(stdout, "{line}")?;
        }
        if self.failures.is_empty() {
            writeln!(stdout, "check-freshness: PASS")?;
            return Ok(0);
        }
        let stderr = io::stderr();
        let mut stderr = stderr.lock();
        writeln!(stderr, "check-freshness: FAIL")?;
        for failure in self.failures {
            writeln!(stderr, "  - {failure}")?;
        }
        Ok(1)
    }
}

pub(super) fn parse_iso_date(value: &Value) -> Option<CivilDate> {
    let text = value.as_str()?;
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return None;
    }
    let year = text[0..4].parse::<i64>().ok()?;
    let month = text[5..7].parse::<u32>().ok()?;
    let day = text[8..10].parse::<u32>().ok()?;
    valid_date(year, month, day).then_some(CivilDate { year, month, day })
}

pub(super) fn parse_timestamp(value: &Value) -> Option<i64> {
    let text = value.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    let normalized = text.replace('Z', "+00:00");
    let separator = normalized.find(['T', 't', ' ']);
    let (date_part, time_part) = separator.map_or((normalized.as_str(), None), |index| {
        (&normalized[..index], Some(&normalized[index + 1..]))
    });
    let date = parse_iso_date(&json!(date_part))?;
    let days = days_from_civil(date.year, date.month, date.day);
    let Some(time_part) = time_part else {
        return Some(days * 86_400_000_000);
    };
    parse_time_micros(time_part).map(|time| days * 86_400_000_000 + time)
}

pub(super) fn current_date(now_micros: i64) -> CivilDate {
    let days = now_micros.div_euclid(86_400_000_000);
    civil_from_days(days)
}

pub(super) fn format_utc(now_micros: i64) -> String {
    let day_micros = now_micros.rem_euclid(86_400_000_000);
    let date = current_date(now_micros);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        date.year,
        date.month,
        date.day,
        day_micros / 3_600_000_000,
        (day_micros / 60_000_000) % 60,
        (day_micros / 1_000_000) % 60
    )
}

pub(super) fn norm_version(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .strip_prefix('v')
        .unwrap_or_else(|| value.and_then(Value::as_str).unwrap_or("").trim())
        .split('+')
        .next()
        .unwrap_or("")
        .to_owned()
}

pub(super) fn py_repr(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".to_owned(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::Bool(false)) => "False".to_owned(),
        Some(Value::String(text)) => {
            let mut quoted = String::from("'");
            for character in text.chars() {
                match character {
                    '\\' => quoted.push_str("\\\\"),
                    '\'' => quoted.push_str("\\'"),
                    '\n' => quoted.push_str("\\n"),
                    '\r' => quoted.push_str("\\r"),
                    '\t' => quoted.push_str("\\t"),
                    other => quoted.push(other),
                }
            }
            quoted.push('\'');
            quoted
        }
        Some(Value::Array(values)) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| py_repr(Some(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some(Value::Object(values)) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!(
                    "{}: {}",
                    py_repr(Some(&json!(key))),
                    py_repr(Some(value))
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some(value) => value.to_string(),
    }
}

fn valid_date(year: i64, month: u32, day: u32) -> bool {
    if !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day <= days
}

fn parse_time_micros(text: &str) -> Option<i64> {
    let (clock, offset) = parse_offset(text)?;
    let mut parts = clock.split(':');
    let hour = parts.next()?.parse::<i64>().ok()?;
    let minute = parts.next()?.parse::<i64>().ok()?;
    let second_part = parts.next().unwrap_or("0");
    if parts.next().is_some() || !(0..=23).contains(&hour) || !(0..=59).contains(&minute) {
        return None;
    }
    let (second_text, fraction) = second_part
        .split_once('.')
        .map_or((second_part, ""), |(seconds, fractional)| {
            (seconds, fractional)
        });
    let second = second_text.parse::<i64>().ok()?;
    if !(0..=59).contains(&second)
        || fraction.len() > 6
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let fractional = if fraction.is_empty() {
        0
    } else {
        format!("{fraction:0<6}").parse::<i64>().ok()?
    };
    Some((((hour * 60 + minute) * 60 + second) * 1_000_000 + fractional) - offset)
}

fn parse_offset(text: &str) -> Option<(&str, i64)> {
    if let Some(index) = text.rfind(['+', '-']) {
        let (clock, sign_and_offset) = text.split_at(index);
        let sign = if sign_and_offset.starts_with('-') {
            -1
        } else {
            1
        };
        let raw = &sign_and_offset[1..];
        let (hours, minutes) = if let Some((hours, minutes)) = raw.split_once(':') {
            (hours, minutes)
        } else if raw.len() == 4 {
            (raw.get(..2)?, raw.get(2..)?)
        } else {
            return None;
        };
        let hours = hours.parse::<i64>().ok()?;
        let minutes = minutes.parse::<i64>().ok()?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        Some((clock, sign * (hours * 60 + minutes) * 60 * 1_000_000))
    } else {
        Some((text, 0))
    }
}

pub(super) fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> CivilDate {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    CivilDate {
        year,
        month: u32::try_from(month).unwrap_or(1),
        day: u32::try_from(day).unwrap_or(1),
    }
}
