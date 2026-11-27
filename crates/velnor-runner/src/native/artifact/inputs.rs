// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::num::{IntErrorKind, ParseIntError};
use std::str::FromStr;

use super::js_text;
use super::name_pattern::NamePattern;
use crate::exec::{ExecError, Inputs};

pub(super) const UPLOAD: &str = "actions/upload-artifact";
pub(super) const DOWNLOAD: &str = "actions/download-artifact";
pub(super) const UPLOAD_INPUTS: [&str; 5] = [
    "name",
    "path",
    "if-no-files-found",
    "retention-days",
    "overwrite",
];
pub(super) const DOWNLOAD_INPUTS: [&str; 2] = ["pattern", "path"];

#[derive(Debug, PartialEq, Eq)]
pub(super) enum NoFiles {
    Warn,
    Error,
    Ignore,
    Unrecognized,
}

#[derive(Debug)]
pub(super) struct UploadInputs {
    pub(super) name: String,
    pub(super) path: String,
    pub(super) if_no_files_found: NoFiles,
    pub(super) retention_days: Option<i64>,
    pub(super) overwrite: bool,
    pub(super) failures: Vec<String>,
}

#[derive(Debug)]
pub(super) struct DownloadInputs {
    pub(super) pattern: Option<NamePattern>,
    pub(super) path: Option<String>,
}

pub(super) fn saturating<T: FromStr<Err = ParseIntError>>(text: &str, max: T) -> Option<T> {
    match text.parse::<T>() {
        Ok(value) => Some(value),
        Err(error) if *error.kind() == IntErrorKind::PosOverflow => Some(max),
        Err(_) => None,
    }
}

pub(super) fn parse_int(text: &str) -> Option<i64> {
    let (negative, unsigned) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (radix, digits) = match unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        Some(hex) => (16, hex),
        None => (10, unsigned),
    };
    let mut found = digits
        .chars()
        .map_while(|digit| digit.to_digit(radix))
        .peekable();
    found.peek()?;
    let magnitude = found.fold(0_i64, |total, digit| {
        total
            .saturating_mul(i64::from(radix))
            .saturating_add(i64::from(digit))
    });
    Some(if negative { -magnitude } else { magnitude })
}

pub(super) fn unsupported(action: &str, known: &[&str], name: &str) -> Option<String> {
    (!known.contains(&name.to_ascii_lowercase().as_str()))
        .then(|| format!("{action} input '{name}' is not supported by velnor-runner"))
}

fn normalized(
    inputs: &Inputs,
    action: &str,
    known: &[&str],
) -> Result<BTreeMap<String, String>, ExecError> {
    let mut found = BTreeMap::new();
    for (name, value) in inputs.iter() {
        if let Some(message) = unsupported(action, known, name) {
            return Err(ExecError::Unsupported(message));
        }
        let lower = name.to_ascii_lowercase();
        found.insert(lower, js_text::trim(value).to_owned());
    }
    Ok(found)
}

fn boolean(value: Option<&String>, name: &str) -> Result<bool, ExecError> {
    match value.map(String::as_str) {
        None | Some("false" | "False" | "FALSE") => Ok(false),
        Some("true" | "True" | "TRUE") => Ok(true),
        Some(_) => Err(ExecError::Invalid(format!(
            "Input does not meet YAML 1.2 \"Core Schema\" specification: {name}\nSupport boolean input list: `true | True | TRUE | false | False | FALSE`"
        ))),
    }
}

impl UploadInputs {
    pub(super) fn parse(inputs: &Inputs) -> Result<Self, ExecError> {
        let found = normalized(inputs, UPLOAD, &UPLOAD_INPUTS)?;
        let path = found
            .get("path")
            .filter(|path| !path.is_empty())
            .ok_or_else(|| ExecError::Invalid("Input required and not supplied: path".to_owned()))?
            .clone();
        let overwrite = boolean(found.get("overwrite"), "overwrite")?;
        let mut failures = Vec::new();
        let if_no_files_found = match found.get("if-no-files-found").map(String::as_str) {
            None | Some("warn") => NoFiles::Warn,
            Some("error") => NoFiles::Error,
            Some("ignore") => NoFiles::Ignore,
            Some(other) => {
                failures.push(format!(
                    "Unrecognized if-no-files-found input. Provided: {other}. Available options: warn,error,ignore"
                ));
                NoFiles::Unrecognized
            }
        };
        let retention_days = match found.get("retention-days").map(String::as_str) {
            None | Some("") => None,
            Some(days) => match parse_int(days) {
                Some(0) => None,
                Some(parsed) => Some(parsed),
                None => {
                    failures.push("Invalid retention-days".to_owned());
                    None
                }
            },
        };
        Ok(Self {
            name: found
                .get("name")
                .cloned()
                .unwrap_or_else(|| "artifact".to_owned()),
            path,
            if_no_files_found,
            retention_days,
            overwrite,
            failures,
        })
    }
}

impl DownloadInputs {
    pub(super) fn parse(inputs: &Inputs) -> Result<Self, ExecError> {
        let mut found = normalized(inputs, DOWNLOAD, &DOWNLOAD_INPUTS)?;
        Ok(Self {
            pattern: found
                .remove("pattern")
                .filter(|value| !value.is_empty())
                .map(|pattern| NamePattern::parse(&pattern).map_err(ExecError::Unsupported))
                .transpose()?,
            path: found.remove("path").filter(|value| !value.is_empty()),
        })
    }
}
