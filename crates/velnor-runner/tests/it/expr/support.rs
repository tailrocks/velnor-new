// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use uuid::Uuid;
use velnor_runner::context_value::ContextValue;
use velnor_runner::expr::{ExprEnv, RootContext, Value, evaluate_expression, evaluate_template};

const LINEAR_TIME_LIMIT: Duration = Duration::from_secs(10);
const MISMATCH_PREFIX_CHARS: usize = 200;

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

pub(crate) fn describe_success(value: &Value) -> String {
    let kind = match value {
        Value::Null => "null",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    format!(
        "kind {kind} with {} bytes as text",
        value.convert_to_string().len()
    )
}

pub(crate) fn error_text(expression: &str, env: &ExprEnv) -> Result<String> {
    match evaluate_expression(expression, env) {
        Ok(value) => bail!("{expression:?} evaluated to {}", describe_success(&value)),
        Err(error) => Ok(error.to_string()),
    }
}

pub(crate) fn template_error_text(source: &str, env: &ExprEnv) -> Result<String> {
    match evaluate_template(source, env) {
        Ok(rendered) => bail!(
            "a template of {} bytes evaluated to {} bytes",
            source.len(),
            rendered.len()
        ),
        Err(error) => Ok(error.to_string()),
    }
}

pub(crate) fn same_text(actual: &str, expected: &str) -> Result<()> {
    if actual == expected {
        return Ok(());
    }
    let prefix = |text: &str| text.chars().take(MISMATCH_PREFIX_CHARS).collect::<String>();
    bail!(
        "text differs: actual has {} chars and starts {:?}; expected has {} chars and starts {:?}",
        actual.chars().count(),
        prefix(actual),
        expected.chars().count(),
        prefix(expected)
    )
}

pub(crate) fn exceeded_memory(expression: &str) -> String {
    format!(
        "The maximum allowed memory size was exceeded while evaluating the following expression: {expression}"
    )
}

pub(crate) fn within_the_linear_time_limit<T: Send + 'static>(
    behaviour: &str,
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(work());
    });
    match receiver.recv_timeout(LINEAR_TIME_LIMIT) {
        Ok(result) => result,
        Err(_) => bail!(
            "{behaviour} did not finish within {} seconds",
            LINEAR_TIME_LIMIT.as_secs()
        ),
    }
}

pub(crate) fn expected_digest(contents: &[&[u8]]) -> String {
    let mut aggregate = Sha256::new();
    for content in contents {
        aggregate.update(Sha256::digest(content));
    }
    aggregate
        .finalize()
        .iter()
        .fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
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

    pub(crate) fn workspace(&self) -> Result<PathBuf> {
        fs::canonicalize(self.root.join("ws")).context("canonicalize workspace")
    }

    pub(crate) fn env(&self) -> ExprEnv {
        ExprEnv::new(self.root.join("ws"))
    }

    pub(crate) fn symlink(&self, link: &str, target: &Path) -> Result<()> {
        let path = self.root.join(link);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("create parent")?;
        }
        std::os::unix::fs::symlink(target, path).context("create symlink")
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
