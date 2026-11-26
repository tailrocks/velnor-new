// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::borrow::Cow;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use regex_automata::meta::Regex;

use crate::job_message::{AgentJobRequestMessage, MaskType};

const MASK: &str = "***";
const BASE64_SHIFTS: [usize; 3] = [0, 1, 2];

#[derive(Debug, Default, Clone)]
pub(super) struct Masker {
    values: Vec<String>,
    patterns: Vec<Regex>,
}

impl Masker {
    pub(super) fn add(&mut self, value: &str) {
        let lines = value
            .split(['\r', '\n'])
            .map(str::trim)
            .filter(|line| !line.is_empty());
        let forms: Vec<&str> = std::iter::once(value).chain(lines).collect();
        for form in forms {
            self.insert(form);
            for shift in BASE64_SHIFTS {
                let bytes = form.as_bytes();
                let shifted = bytes.get(shift..).filter(|rest| !rest.is_empty());
                self.insert(&STANDARD.encode(shifted.unwrap_or(bytes)));
            }
        }
    }

    pub(super) fn add_pattern(&mut self, pattern: &str) {
        self.add(pattern);
        if let Ok(regex) = Regex::new(pattern) {
            self.patterns.push(regex);
        }
    }

    pub(super) fn add_access_token(&mut self, token: &str) {
        self.add(token);
        self.add(&format!("x-access-token:{token}"));
    }

    fn insert(&mut self, value: &str) {
        if value.trim().is_empty() || self.values.iter().any(|known| known == value) {
            return;
        }
        self.values.push(value.to_owned());
        self.values
            .sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    }

    pub(super) fn mask<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let mut masked = Cow::Borrowed(text);
        for value in &self.values {
            if masked.contains(value.as_str()) {
                masked = Cow::Owned(masked.replace(value.as_str(), MASK));
            }
        }
        for regex in &self.patterns {
            masked = replace_matches(masked, regex);
        }
        masked
    }

    pub(super) fn reveals(&self, text: &str) -> bool {
        matches!(self.mask(text), Cow::Owned(_))
    }
}

fn replace_matches<'a>(text: Cow<'a, str>, regex: &Regex) -> Cow<'a, str> {
    let mut replaced = String::new();
    let mut last = 0;
    for found in regex
        .find_iter(text.as_ref())
        .filter(|found| !found.is_empty())
    {
        replaced.push_str(&text[last..found.start()]);
        replaced.push_str(MASK);
        last = found.end();
    }
    if last == 0 {
        return text;
    }
    replaced.push_str(&text[last..]);
    Cow::Owned(replaced)
}

pub(super) fn for_job(message: &AgentJobRequestMessage) -> Masker {
    let mut masker = Masker::default();
    for hint in &message.mask {
        let Some(value) = hint.value.as_ref().map(|value| value.expose().as_str()) else {
            continue;
        };
        if hint.kind == Some(MaskType::Regex) {
            masker.add_pattern(value);
        } else {
            masker.add(value);
        }
    }
    for (name, variable) in &message.variables {
        let Some(value) = variable
            .value
            .as_ref()
            .map(|value| value.expose().as_str())
            .filter(|_| variable.is_secret)
        else {
            continue;
        };
        if name.eq_ignore_ascii_case("system.github.token")
            || name.eq_ignore_ascii_case("system.accesstoken")
        {
            masker.add_access_token(value);
        } else {
            masker.add(value);
        }
    }
    let parameters = message
        .resources
        .endpoints
        .iter()
        .filter_map(|endpoint| endpoint.authorization.as_ref())
        .flat_map(|authorization| authorization.parameters.values())
        .flatten();
    for value in parameters {
        masker.add(value.expose());
    }
    masker
}
