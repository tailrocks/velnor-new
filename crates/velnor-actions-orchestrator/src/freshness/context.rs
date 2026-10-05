//! Shared freshness state, row output, and date parsing.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

pub(super) use super::dates::{
    CivilDate, current_date, days_from_civil, format_utc, parse_iso_date, parse_timestamp,
};
use super::rust_source::extract_public_string_const;

pub(super) const RUST_SOURCE_CAP: usize = 256 * 1024;

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
    pub(super) workspace_roots: Vec<PathBuf>,
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
            workspace_roots: Vec::new(),
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
