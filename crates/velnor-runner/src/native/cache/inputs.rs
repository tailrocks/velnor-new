// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use std::collections::BTreeMap;

use crate::broker::cache::{MAX_KEY_LENGTH, key_units};
use crate::exec::{ExecError, Inputs};

const MAX_KEYS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CacheKind {
    Restore,
    Save,
}

impl CacheKind {
    pub(super) fn of(reference: &str) -> Option<Self> {
        match reference
            .split_once('@')
            .map_or(reference, |(name, _)| name)
        {
            "actions/cache/restore" => Some(Self::Restore),
            "actions/cache/save" => Some(Self::Save),
            _ => None,
        }
    }

    pub(super) fn revision(reference: &str) -> &str {
        reference
            .split_once('@')
            .map_or("", |(_, revision)| revision)
    }

    fn inputs(self) -> &'static [&'static str] {
        match self {
            Self::Restore => &["path", "key", "restore-keys", "lookup-only"],
            Self::Save => &["path", "key"],
        }
    }
}

pub(super) const SUPPORTED_REVISION: &str = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9";

pub(super) fn check_revision(reference: &str) -> Result<(), ExecError> {
    match CacheKind::revision(reference) {
        SUPPORTED_REVISION => Ok(()),
        other => Err(ExecError::Unsupported(format!(
            "actions/cache revision '{other}' is not supported by velnor-runner, which implements {SUPPORTED_REVISION}"
        ))),
    }
}

#[derive(Debug, Clone)]
pub(super) struct CacheInputs {
    pub(super) paths: Vec<String>,
    pub(super) key: String,
    pub(super) restore_keys: Vec<String>,
    pub(super) lookup_only: bool,
}

fn lines(value: Option<&&String>) -> Vec<String> {
    value
        .map(|text| text.lines().map(str::trim))
        .into_iter()
        .flatten()
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(super) fn invalid(message: impl Into<String>) -> ExecError {
    ExecError::Invalid(message.into())
}

impl CacheInputs {
    pub(super) fn parse(inputs: &Inputs, kind: CacheKind) -> Result<Self, ExecError> {
        let inputs: BTreeMap<String, &String> = inputs
            .iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value))
            .collect();
        if let Some(name) = inputs
            .keys()
            .find(|name| !kind.inputs().contains(&name.as_str()))
        {
            return Err(ExecError::Unsupported(format!(
                "actions/cache input '{name}' is not supported by velnor-runner"
            )));
        }
        let key = inputs.get("key").map_or("", |key| key.trim()).to_owned();
        if key.is_empty() {
            return Err(invalid(match kind {
                CacheKind::Save => "Key is not specified.",
                CacheKind::Restore => "Input required and not supplied: key",
            }));
        }
        let paths = lines(inputs.get("path"));
        if paths.is_empty() {
            return Err(invalid("Input required and not supplied: path"));
        }
        if let Some(path) = paths
            .iter()
            .find(|path| path.starts_with('!') || path.contains(['*', '?', '[']))
        {
            return Err(ExecError::Unsupported(format!(
                "actions/cache path '{path}' uses a glob"
            )));
        }
        let restore_keys = lines(inputs.get("restore-keys"));
        if restore_keys.len() >= MAX_KEYS {
            return Err(invalid(format!(
                "Key Validation Error: Keys are limited to a maximum of {MAX_KEYS}."
            )));
        }
        for checked in std::iter::once(&key).chain(&restore_keys) {
            if key_units(checked) > MAX_KEY_LENGTH {
                return Err(invalid(format!(
                    "Key Validation Error: {checked} cannot be larger than {MAX_KEY_LENGTH} characters."
                )));
            }
            if checked.contains(',') {
                return Err(invalid(format!(
                    "Key Validation Error: {checked} cannot contain commas."
                )));
            }
        }
        Ok(Self {
            paths,
            key,
            restore_keys,
            lookup_only: inputs
                .get("lookup-only")
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("true")),
        })
    }
}
