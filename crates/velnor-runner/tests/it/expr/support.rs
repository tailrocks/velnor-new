// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use velnor_runner::context_value::ContextValue;
use velnor_runner::expr::{ExprEnv, RootContext, Value, evaluate_expression};

pub(crate) fn env_with(contexts: &[(RootContext, &str)]) -> Result<ExprEnv> {
    let mut env = ExprEnv::new(PathBuf::from("/nonexistent-workspace"));
    for (root, json) in contexts {
        let value = ContextValue::from_json_str(json).context("parse context json")?;
        env.set_context(*root, &value);
    }
    Ok(env)
}

pub(crate) fn empty_env() -> ExprEnv {
    ExprEnv::new(PathBuf::from("/nonexistent-workspace"))
}

pub(crate) fn eval(expression: &str, env: &ExprEnv) -> Result<Value> {
    Ok(evaluate_expression(expression, env)?)
}

pub(crate) fn text(expression: &str, env: &ExprEnv) -> Result<String> {
    Ok(eval(expression, env)?.convert_to_string())
}

pub(crate) fn truthy(expression: &str, env: &ExprEnv) -> Result<bool> {
    Ok(eval(expression, env)?.is_truthy())
}

pub(crate) fn error_text(expression: &str, env: &ExprEnv) -> Result<String> {
    match evaluate_expression(expression, env) {
        Ok(value) => anyhow::bail!("{expression:?} evaluated to {value:?}"),
        Err(error) => Ok(error.to_string()),
    }
}

#[derive(Debug)]
pub(crate) struct TempDir {
    root: PathBuf,
}

impl TempDir {
    pub(crate) fn new() -> Result<Self> {
        let root = std::env::temp_dir().join(format!("velnor-expr-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).context("create temp dir")?;
        Ok(Self { root })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.root
    }

    pub(crate) fn write(&self, relative: &str, contents: &str) -> Result<()> {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("create parent")?;
        }
        fs::write(path, contents).context("write file")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
