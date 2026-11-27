// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::borrow::Cow;

use regex_automata::meta::Regex;

use super::encode::ENCODERS;
use super::{Annotation, ExecError};
use crate::job_message::{AgentJobRequestMessage, MaskKind};

const MASK: &str = "***";
const SECRET_WHITELIST: [&str; 2] = ["ACTIONS_STEP_DEBUG", "ACTIONS_RUNNER_DEBUG"];

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
            for encode in ENCODERS {
                self.insert(&encode(form));
            }
        }
    }

    pub(super) fn add_pattern(&mut self, pattern: &str) -> bool {
        let Ok(regex) = Regex::new(pattern) else {
            return false;
        };
        self.add(pattern);
        self.patterns.push(regex);
        true
    }

    pub(super) fn add_access_token(&mut self, token: &str) {
        self.add(token);
        self.add(&format!("x-access-token:{token}"));
    }

    fn insert(&mut self, value: &str) {
        if value.is_empty() || self.values.iter().any(|known| known == value) {
            return;
        }
        self.values.push(value.to_owned());
    }

    pub(super) fn ranges(&self, text: &str) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        for value in &self.values {
            value_spans(text, value, &mut spans);
        }
        for regex in &self.patterns {
            spans.extend(
                regex
                    .find_iter(text)
                    .filter(|found| !found.is_empty())
                    .map(|found| (found.start(), found.end())),
            );
        }
        spans.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
        for (start, end) in spans {
            match merged.last_mut() {
                Some(last) if start <= last.1 => last.1 = last.1.max(end),
                _ => merged.push((start, end)),
            }
        }
        merged
    }

    pub(super) fn mask<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let ranges = self.ranges(text);
        if ranges.is_empty() {
            return Cow::Borrowed(text);
        }
        Cow::Owned(redact(text, &ranges, text.len()))
    }

    pub(super) fn reveals(&self, text: &str) -> bool {
        matches!(self.mask(text), Cow::Owned(_))
    }
}

pub(super) fn redact(text: &str, ranges: &[(usize, usize)], end: usize) -> String {
    let mut masked = String::with_capacity(end);
    let mut copied = 0;
    for &(start, stop) in ranges.iter().take_while(|(start, _)| *start < end) {
        masked.push_str(&text[copied..start]);
        masked.push_str(MASK);
        copied = stop;
    }
    if copied < end {
        masked.push_str(&text[copied..end]);
    }
    masked
}

fn value_spans(text: &str, value: &str, spans: &mut Vec<(usize, usize)>) {
    let mut from = 0;
    while let Some(found) = text[from..].find(value) {
        let start = from + found;
        spans.push((start, start + value.len()));
        from = start + text[start..].chars().next().map_or(1, char::len_utf8);
    }
}

pub(super) fn mask_annotation(masker: &Masker, annotation: &mut Annotation) {
    for text in [&mut annotation.message]
        .into_iter()
        .chain(annotation.title.as_mut())
        .chain(annotation.file.as_mut())
    {
        if let Cow::Owned(masked) = masker.mask(text) {
            *text = masked;
        }
    }
}

pub(super) fn load(masker: &mut Masker, message: &AgentJobRequestMessage) -> Result<(), ExecError> {
    let hints =
        || {
            message.mask.iter().enumerate().filter_map(|(index, hint)| {
                Some((index, hint, hint.value.as_ref()?.expose().as_str()))
            })
        };
    for (_, hint, value) in hints() {
        if hint.kind != MaskKind::Regex {
            masker.add(value);
        }
    }
    for (name, variable) in &message.variables {
        if SECRET_WHITELIST
            .iter()
            .any(|known| known.eq_ignore_ascii_case(name))
        {
            continue;
        }
        let Some(value) = variable
            .value
            .as_ref()
            .map(|value| value.expose().trim())
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
    for (index, hint, value) in hints() {
        if hint.kind == MaskKind::Regex && !masker.add_pattern(value) {
            return Err(ExecError::Invalid(format!(
                "mask hint {} is not a valid regular expression",
                index + 1
            )));
        }
    }
    Ok(())
}
