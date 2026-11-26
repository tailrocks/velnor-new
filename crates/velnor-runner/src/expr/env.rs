// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::eval::from_context_value;
use super::value::{ObjectValue, Value};
use crate::context_value::ContextValue;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RootContext {
    Github,
    Runner,
    Needs,
    Steps,
    Inputs,
    Secrets,
    Env,
}

impl RootContext {
    const ALL: [(&'static str, Self); 7] = [
        ("github", Self::Github),
        ("runner", Self::Runner),
        ("needs", Self::Needs),
        ("steps", Self::Steps),
        ("inputs", Self::Inputs),
        ("secrets", Self::Secrets),
        ("env", Self::Env),
    ];

    fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
            .map(|(_, root)| *root)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Function {
    Format,
    Contains,
    StartsWith,
    ToJson,
    FromJson,
    HashFiles,
    Success,
    Always,
    Cancelled,
}

impl Function {
    const ALL: [(&'static str, Self, usize, usize); 9] = [
        ("format", Self::Format, 1, 255),
        ("contains", Self::Contains, 2, 2),
        ("startsWith", Self::StartsWith, 2, 2),
        ("toJSON", Self::ToJson, 1, 1),
        ("fromJSON", Self::FromJson, 1, 1),
        ("hashFiles", Self::HashFiles, 1, 255),
        ("success", Self::Success, 0, 0),
        ("always", Self::Always, 0, 0),
        ("cancelled", Self::Cancelled, 0, 0),
    ];

    fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(known, ..)| known.eq_ignore_ascii_case(name))
            .map(|(_, function, ..)| *function)
    }

    pub(crate) fn arity(self) -> (usize, usize) {
        Self::ALL
            .iter()
            .find(|(_, function, ..)| *function == self)
            .map_or((0, 0), |(_, _, min, max)| (*min, *max))
    }
}

pub(crate) trait ParseEnvironment {
    fn root_context(&self, name: &str) -> Option<RootContext>;

    fn function(&self, name: &str) -> Option<Function>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Success,
    Failure,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct ExprEnv {
    pub workspace: PathBuf,
    pub home: Option<PathBuf>,
    pub status: JobStatus,
    contexts: BTreeMap<RootContext, Value>,
}

/// Mirrors node's `os.homedir()`: a set `HOME` wins as is, even when empty, and
/// only an unset one falls back to the passwd entry.
pub(crate) fn home_dir(var: Option<std::ffi::OsString>) -> Option<PathBuf> {
    var.map(PathBuf::from).or_else(std::env::home_dir)
}

impl ExprEnv {
    #[must_use]
    pub fn new(workspace: PathBuf) -> Self {
        let contexts = RootContext::ALL
            .iter()
            .map(|(_, root)| (*root, Value::Object(ObjectValue::new(Vec::new()))))
            .collect();
        Self {
            workspace,
            home: home_dir(std::env::var_os("HOME")),
            status: JobStatus::Success,
            contexts,
        }
    }

    pub fn set_context(&mut self, root: RootContext, value: &ContextValue) {
        self.contexts.insert(root, from_context_value(value));
    }

    pub(crate) fn context(&self, root: RootContext) -> Value {
        self.contexts.get(&root).cloned().unwrap_or(Value::Null)
    }
}

impl ParseEnvironment for ExprEnv {
    fn root_context(&self, name: &str) -> Option<RootContext> {
        RootContext::parse(name)
    }

    fn function(&self, name: &str) -> Option<Function> {
        Function::parse(name)
    }
}

#[cfg(test)]
mod tests {
    use super::home_dir;
    use std::path::PathBuf;

    #[test]
    fn expr_home_keeps_a_set_variable_even_when_empty() {
        assert_eq!(home_dir(Some("".into())), Some(PathBuf::new()));
        assert_eq!(home_dir(Some("rel".into())), Some(PathBuf::from("rel")));
    }
}
