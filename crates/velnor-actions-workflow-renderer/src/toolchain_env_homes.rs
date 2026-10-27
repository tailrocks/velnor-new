// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::RenderError;

use super::{reject_denied_step_keys, reject_denied_tf_keys, reject_privileged_task_keys};

pub const TOOLCHAIN_HOME_KEYS: [&str; 5] = [
    "MISE_RUSTUP_HOME",
    "MISE_CARGO_HOME",
    "RUSTUP_TOOLCHAIN",
    "RUSTUP_HOME",
    "CARGO_HOME",
];

pub const SHARED_WORKSPACE_ROOT: &str = "$GITHUB_WORKSPACE";
pub const SHARED_OWNED_ROOT: &str = "${{ runner.temp }}/velnor-gc1";
pub const SHARED_SOURCE_ROOT: &str = "${{ runner.temp }}/velnor-gc1/sources";
pub const SHARED_TOOL_ROOT: &str = "${{ runner.temp }}/velnor-gc1/tools";
pub const SHARED_CARGO_HOME: &str = "${{ runner.temp }}/velnor-gc1/cargo";
pub const SHARED_RUSTUP_HOME: &str = "${{ runner.temp }}/velnor-gc1/rust-home";
pub const SHARED_TRANSPORT_ROOT: &str = "${{ runner.temp }}/velnor-gc1/transport";
pub const SHARED_OBSERVATION_ROOT: &str = "${{ runner.temp }}/velnor-gc1/observations";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedRunnerHomes {
    pub workspace_root: String,
    pub owned_root: String,
    pub source_root: String,
    pub tool_root: String,
    pub cargo_home: String,
    pub rustup_home: String,
    pub transport_root: String,
    pub observation_root: String,
}

impl SharedRunnerHomes {
    #[must_use]
    pub fn github() -> Self {
        Self {
            workspace_root: SHARED_WORKSPACE_ROOT.to_owned(),
            owned_root: SHARED_OWNED_ROOT.to_owned(),
            source_root: SHARED_SOURCE_ROOT.to_owned(),
            tool_root: SHARED_TOOL_ROOT.to_owned(),
            cargo_home: SHARED_CARGO_HOME.to_owned(),
            rustup_home: SHARED_RUSTUP_HOME.to_owned(),
            transport_root: SHARED_TRANSPORT_ROOT.to_owned(),
            observation_root: SHARED_OBSERVATION_ROOT.to_owned(),
        }
    }

    pub fn validate(&self) -> Result<(), RenderError> {
        if self.workspace_root != SHARED_WORKSPACE_ROOT {
            return Err(RenderError::BadCommand(format!(
                "shared_home_workspace:{}",
                self.workspace_root
            )));
        }
        if self.owned_root != SHARED_OWNED_ROOT {
            return Err(RenderError::BadCommand(format!(
                "shared_home_owned_root:{}",
                self.owned_root
            )));
        }
        for (field, value) in [
            ("source_root", &self.source_root),
            ("tool_root", &self.tool_root),
            ("cargo_home", &self.cargo_home),
            ("rustup_home", &self.rustup_home),
            ("transport_root", &self.transport_root),
            ("observation_root", &self.observation_root),
        ] {
            check_shared_root(field, value, &self.owned_root)?;
        }
        let roots = [
            self.workspace_root.as_str(),
            self.owned_root.as_str(),
            self.source_root.as_str(),
            self.tool_root.as_str(),
            self.cargo_home.as_str(),
            self.rustup_home.as_str(),
            self.transport_root.as_str(),
            self.observation_root.as_str(),
        ];
        let mut distinct = std::collections::BTreeSet::new();
        for root in roots {
            if !distinct.insert(root) {
                return Err(RenderError::BadCommand(format!(
                    "shared_home_overlap:{root}"
                )));
            }
        }
        Ok(())
    }

    pub fn checked_env(
        &self,
        base: &BTreeMap<String, String>,
        toolchain: &str,
    ) -> Result<BTreeMap<String, String>, RenderError> {
        self.validate()?;
        reject_privileged_task_keys(base)?;
        reject_denied_tf_keys(base)?;
        reject_conflicting_home_assignments(base)?;
        let merged = with_toolchain_homes(base, &self.rustup_home, &self.cargo_home, toolchain);
        check_toolchain_homes(&merged)?;
        reject_denied_step_keys(&merged)?;
        Ok(merged)
    }
}

fn check_shared_root(field: &str, value: &str, owned_root: &str) -> Result<(), RenderError> {
    if value.is_empty() {
        return Err(RenderError::BadCommand(format!(
            "shared_home_empty:{field}"
        )));
    }
    if value
        .chars()
        .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        return Err(RenderError::BadCommand(format!(
            "shared_home_control:{field}"
        )));
    }
    if value.contains('`') || value.contains("$(") {
        return Err(RenderError::BadCommand(format!(
            "shared_home_substitution:{field}"
        )));
    }
    if value.split('/').any(|segment| segment == "..") {
        return Err(RenderError::BadCommand(format!(
            "shared_home_escape:{field}"
        )));
    }
    let Some(suffix) = value.strip_prefix(owned_root) else {
        return Err(RenderError::BadCommand(format!(
            "shared_home_outside_owned:{field}"
        )));
    };
    if !suffix.starts_with('/') {
        return Err(RenderError::BadCommand(format!(
            "shared_home_outside_owned:{field}"
        )));
    }
    Ok(())
}

pub fn reject_conflicting_home_assignments(
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if env.contains_key("HOME") {
        return Err(RenderError::BadCommand(
            "disposable_home_assignment:HOME".to_owned(),
        ));
    }
    Ok(())
}

#[must_use]
pub fn with_toolchain_homes(
    env: &BTreeMap<String, String>,
    rustup_home: &str,
    cargo_home: &str,
    toolchain: &str,
) -> BTreeMap<String, String> {
    let mut merged = env.clone();
    merged.insert(TOOLCHAIN_HOME_KEYS[0].to_owned(), rustup_home.to_owned());
    merged.insert(TOOLCHAIN_HOME_KEYS[1].to_owned(), cargo_home.to_owned());
    merged.insert(TOOLCHAIN_HOME_KEYS[3].to_owned(), rustup_home.to_owned());
    merged.insert(TOOLCHAIN_HOME_KEYS[4].to_owned(), cargo_home.to_owned());
    if !toolchain.is_empty() {
        merged.insert(TOOLCHAIN_HOME_KEYS[2].to_owned(), toolchain.to_owned());
    }
    merged
}

pub fn check_toolchain_homes(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for key in [
        TOOLCHAIN_HOME_KEYS[0],
        TOOLCHAIN_HOME_KEYS[1],
        TOOLCHAIN_HOME_KEYS[3],
        TOOLCHAIN_HOME_KEYS[4],
    ] {
        if env.get(key).is_none_or(String::is_empty) {
            return Err(RenderError::BadCommand(format!(
                "missing_toolchain_home:{key}"
            )));
        }
    }
    for (mise_key, direct_key) in [
        (TOOLCHAIN_HOME_KEYS[0], TOOLCHAIN_HOME_KEYS[3]),
        (TOOLCHAIN_HOME_KEYS[1], TOOLCHAIN_HOME_KEYS[4]),
    ] {
        if env.get(mise_key) != env.get(direct_key) {
            return Err(RenderError::BadCommand(format!(
                "conflicting_toolchain_home:{mise_key}:{direct_key}"
            )));
        }
    }
    if env
        .get(TOOLCHAIN_HOME_KEYS[2])
        .is_some_and(String::is_empty)
    {
        return Err(RenderError::BadCommand(format!(
            "missing_toolchain_home:{}",
            TOOLCHAIN_HOME_KEYS[2]
        )));
    }
    Ok(())
}

pub fn checked_project_task_env(
    base: &BTreeMap<String, String>,
    rustup_home: &str,
    cargo_home: &str,
    toolchain: &str,
) -> Result<BTreeMap<String, String>, RenderError> {
    reject_privileged_task_keys(base)?;
    reject_denied_tf_keys(base)?;
    checked_task_env(base, rustup_home, cargo_home, toolchain)
}

pub fn checked_task_env(
    base: &BTreeMap<String, String>,
    rustup_home: &str,
    cargo_home: &str,
    toolchain: &str,
) -> Result<BTreeMap<String, String>, RenderError> {
    reject_conflicting_home_assignments(base)?;
    for (key, value) in [
        (TOOLCHAIN_HOME_KEYS[0], rustup_home),
        (TOOLCHAIN_HOME_KEYS[1], cargo_home),
    ] {
        if value.is_empty() {
            return Err(RenderError::BadCommand(format!(
                "missing_toolchain_home:{key}"
            )));
        }
    }
    let merged = with_toolchain_homes(base, rustup_home, cargo_home, toolchain);
    check_toolchain_homes(&merged)?;
    reject_denied_step_keys(&merged)?;
    Ok(merged)
}
