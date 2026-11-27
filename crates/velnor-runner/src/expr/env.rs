// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::eval::from_context_value;
use super::seam::Seam;
use super::value::{ObjectValue, Value};
use crate::context_value::{ContextValue, ContextValueError};
use crate::secret::Secret;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RootContext {
    Github,
    Runner,
    Needs,
    Steps,
    Inputs,
    Secrets,
    Env,
    Vars,
}

impl RootContext {
    const ALL: [(&'static str, Self); 8] = [
        ("github", Self::Github),
        ("runner", Self::Runner),
        ("needs", Self::Needs),
        ("steps", Self::Steps),
        ("inputs", Self::Inputs),
        ("secrets", Self::Secrets),
        ("env", Self::Env),
        ("vars", Self::Vars),
    ];

    const SERVICE_OWNED: [Self; 4] = [Self::Github, Self::Needs, Self::Inputs, Self::Vars];

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
    EndsWith,
    ToJson,
    FromJson,
    HashFiles,
    Success,
    Always,
    Cancelled,
    Failure,
}

impl Function {
    const ALL: [(&'static str, Self, usize, usize); 11] = [
        ("format", Self::Format, 1, 255),
        ("contains", Self::Contains, 2, 2),
        ("startsWith", Self::StartsWith, 2, 2),
        ("endsWith", Self::EndsWith, 2, 2),
        ("toJSON", Self::ToJson, 1, 1),
        ("fromJSON", Self::FromJson, 1, 1),
        ("hashFiles", Self::HashFiles, 1, 255),
        ("success", Self::Success, 0, 0),
        ("always", Self::Always, 0, 0),
        ("cancelled", Self::Cancelled, 0, 0),
        ("failure", Self::Failure, 0, 0),
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

const HASH_FILES_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct ExprEnv {
    pub workspace: PathBuf,
    pub home: Option<PathBuf>,
    pub status: JobStatus,
    pub hash_files_timeout: Duration,
    pub(crate) walk_seam: Seam,
    contexts: Secret<BTreeMap<RootContext, Value>>,
}

impl ExprEnv {
    #[must_use]
    pub fn new(workspace: PathBuf) -> Self {
        Self::with_home_var(workspace, std::env::var_os("HOME"))
    }

    #[must_use]
    pub fn with_home_var(workspace: PathBuf, home_var: Option<std::ffi::OsString>) -> Self {
        let contexts = Secret::new(
            RootContext::ALL
                .iter()
                .map(|(_, root)| (*root, Value::Object(ObjectValue::new(Vec::new()))))
                .collect(),
        );
        Self {
            workspace,
            home: home_var.map(PathBuf::from).or_else(std::env::home_dir),
            status: JobStatus::Success,
            hash_files_timeout: HASH_FILES_TIMEOUT,
            walk_seam: Seam::default(),
            contexts,
        }
    }

    pub fn set_context(
        &mut self,
        root: RootContext,
        value: &ContextValue,
    ) -> Result<(), ContextValueError> {
        value.validate()?;
        let mut contexts = std::mem::take(&mut self.contexts).into_inner();
        contexts.insert(root, from_context_value(value));
        self.contexts = Secret::new(contexts);
        Ok(())
    }

    #[cfg(feature = "test-support")]
    pub fn set_walk_seam(&mut self, hook: impl Fn(&std::path::Path) + Send + Sync + 'static) {
        self.walk_seam = Seam::new(hook);
    }

    pub fn set_job_contexts(
        &mut self,
        contexts: &BTreeMap<String, ContextValue>,
    ) -> Result<(), ContextValueError> {
        for (name, value) in contexts {
            if let Some(root) =
                RootContext::parse(name).filter(|root| RootContext::SERVICE_OWNED.contains(root))
            {
                self.set_context(root, value)?;
            }
        }
        Ok(())
    }

    pub fn merge_strings(&mut self, root: RootContext, pairs: &[(String, String)]) {
        let merged = match self.context(root) {
            Value::Object(object) => object.with_strings(pairs),
            _ => ObjectValue::new(Vec::new()).with_strings(pairs),
        };
        self.update_contexts(|contexts| {
            contexts.insert(root, Value::Object(merged));
        });
    }

    pub fn withhold(&mut self, root: RootContext) {
        self.update_contexts(|contexts| {
            contexts.remove(&root);
        });
    }

    fn update_contexts(&mut self, update: impl FnOnce(&mut BTreeMap<RootContext, Value>)) {
        let mut contexts = std::mem::take(&mut self.contexts).into_inner();
        update(&mut contexts);
        self.contexts = Secret::new(contexts);
    }

    pub(crate) fn context(&self, root: RootContext) -> Value {
        self.contexts
            .expose()
            .get(&root)
            .cloned()
            .unwrap_or(Value::Null)
    }
}

impl ParseEnvironment for ExprEnv {
    fn root_context(&self, name: &str) -> Option<RootContext> {
        RootContext::parse(name).filter(|root| self.contexts.expose().contains_key(root))
    }

    fn function(&self, name: &str) -> Option<Function> {
        Function::parse(name)
    }
}

#[cfg(test)]
mod tests {
    use super::ExprEnv;
    use std::path::PathBuf;

    #[test]
    fn expr_home_keeps_a_set_variable_even_when_empty() {
        let home = |var: &str| ExprEnv::with_home_var(PathBuf::new(), Some(var.into())).home;
        assert_eq!(home(""), Some(PathBuf::new()));
        assert_eq!(home("rel"), Some(PathBuf::from("rel")));
    }
}
