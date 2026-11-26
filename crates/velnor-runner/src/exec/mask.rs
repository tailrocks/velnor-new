// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/runner.rs

use std::borrow::Cow;

use regex_automata::meta::Regex;

use super::encode::ENCODERS;
use super::{Annotation, ExecError};
use crate::job_message::{AgentJobRequestMessage, MaskType};

const MASK: &str = "***";

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
        if value.trim().is_empty() || self.values.iter().any(|known| known == value) {
            return;
        }
        self.values.push(value.to_owned());
    }

    pub(super) fn mask<'a>(&self, text: &'a str) -> Cow<'a, str> {
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
        if spans.is_empty() {
            return Cow::Borrowed(text);
        }
        spans.sort_unstable();
        let mut masked = String::with_capacity(text.len());
        let mut copied = 0;
        let mut spans = spans.into_iter();
        let mut current = spans.next().unwrap_or_default();
        for (start, end) in spans.chain(std::iter::once((usize::MAX, usize::MAX))) {
            if start <= current.1 {
                current.1 = current.1.max(end);
                continue;
            }
            masked.push_str(&text[copied..current.0]);
            masked.push_str(MASK);
            copied = current.1;
            current = (start, end);
        }
        masked.push_str(&text[copied..]);
        Cow::Owned(masked)
    }

    pub(super) fn reveals(&self, text: &str) -> bool {
        matches!(self.mask(text), Cow::Owned(_))
    }
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
        if hint.kind != Some(MaskType::Regex) {
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
    for (index, hint, value) in hints() {
        if hint.kind == Some(MaskType::Regex) && !masker.add_pattern(value) {
            return Err(ExecError::Invalid(format!(
                "mask hint {} is not a valid regular expression",
                index + 1
            )));
        }
    }
    Ok(())
}
