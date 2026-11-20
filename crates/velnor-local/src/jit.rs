// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

const RUNNER_LABELS: &str = "ubuntu-latest,ubuntu-22.04,ubuntu-24.04,ubuntu-26.04,ubuntu-22.04-arm,ubuntu-24.04-arm,ubuntu-26.04-arm";

pub const FILES: &[&str] = &[".runner", ".credentials", ".credentials_rsaparams"];

#[must_use]
pub fn labels(extra: &[String]) -> String {
    let mut all: Vec<&str> = RUNNER_LABELS.split(',').collect();
    for label in extra {
        if !all.contains(&label.as_str()) {
            all.push(label);
        }
    }
    all.join(",")
}

#[must_use]
pub fn register_script(name: &str, labels: &str) -> String {
    let dump = FILES
        .iter()
        .map(|file| format!("printf '%s ' {file}; base64 -w0 {file}; echo"))
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "cd /home/runner && ./config.sh --unattended --ephemeral \
         --url http://github.localhost/runner/server --token anything \
         --name {name} --labels {labels} >&2 && {dump}"
    )
}

pub fn pack(output: &str) -> Result<String, String> {
    let mut files = BTreeMap::new();
    for line in output.lines() {
        let (file, content) = line
            .split_once(' ')
            .ok_or_else(|| format!("unexpected registration output: {line}"))?;
        if !FILES.contains(&file) {
            return Err(format!("unexpected registration file {file}"));
        }
        STANDARD
            .decode(content)
            .map_err(|error| format!("{file} is not base64: {error}"))?;
        files.insert(file, content);
    }
    for file in FILES.iter().copied() {
        if !files.contains_key(file) {
            return Err(format!("registration wrote no {file}"));
        }
    }
    let json = serde_json::to_string(&files).map_err(|error| error.to_string())?;
    Ok(STANDARD.encode(json))
}
