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
const MISMATCH_WINDOW_CHARS: usize = 100;
const OUTCOME_TAIL_CHARS: usize = 100;

pub(crate) fn env_with(contexts: &[(RootContext, &str)]) -> Result<ExprEnv> {
    let mut env = ExprEnv::new(PathBuf::from("/nonexistent-workspace"));
    for (root, json) in contexts {
        let value = ContextValue::from_json_str(json).context("parse context json")?;
        env.set_context(*root, &value)?;
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

pub(crate) fn outcome(expression: &str, env: &ExprEnv) -> String {
    match evaluate_expression(expression, env) {
        Ok(value) => format!("ok: {}", value.convert_to_string()),
        Err(error) => {
            let message = error.to_string();
            let skipped = message.chars().count().saturating_sub(OUTCOME_TAIL_CHARS);
            format!(
                "error ending: {}",
                message.chars().skip(skipped).collect::<String>()
            )
        }
    }
}

pub(crate) fn hash_outcome(env: &mut ExprEnv, pattern: &str) -> Result<String> {
    let json = serde_json::json!({ "p": pattern }).to_string();
    env.set_context(RootContext::Env, &ContextValue::from_json_str(&json)?)?;
    Ok(outcome("hashFiles(env.p)", env))
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
    let index = actual
        .chars()
        .zip(expected.chars())
        .take_while(|(left, right)| left == right)
        .count();
    let window = |text: &str| {
        text.chars()
            .skip(index.saturating_sub(MISMATCH_WINDOW_CHARS / 2))
            .take(MISMATCH_WINDOW_CHARS)
            .collect::<String>()
    };
    bail!(
        "text differs at char {index}: actual has {} chars and reads {:?} there; expected has {} chars and reads {:?} there",
        actual.chars().count(),
        window(actual),
        expected.chars().count(),
        window(expected)
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

#[test]
fn support_same_text_shows_the_first_difference_far_from_the_start() {
    let expected = "x".repeat(5_000);
    let actual = format!("{}y{}", "x".repeat(3_000), "x".repeat(1_999));

    let message = same_text(&actual, &expected)
        .expect_err("different texts")
        .to_string();

    assert!(
        message.starts_with("text differs at char 3000:"),
        "{message}"
    );
    assert!(
        message.contains(&format!("{}y{}", "x".repeat(50), "x".repeat(49))),
        "{message}"
    );
    assert!(message.len() < 500, "{} bytes", message.len());
}

#[test]
fn support_outcome_keeps_a_failure_about_a_long_pattern_short() -> Result<()> {
    let mut env = empty_env();
    let pattern = format!("*{}", "a".repeat(70_000));
    env.set_context(
        RootContext::Env,
        &ContextValue::from_json_str(&serde_json::json!({ "p": pattern }).to_string())?,
    )?;

    let message = outcome("hashFiles(env.p)", &env);

    assert!(message.len() < 300, "{} bytes", message.len());
    assert!(message.starts_with("error ending: "), "{message}");
    Ok(())
}
