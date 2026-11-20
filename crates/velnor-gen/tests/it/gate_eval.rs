// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A small evaluator of the `if:` expressions the generator writes, so a test can
//! run a rendered condition against the outcomes a job could have.

use std::collections::BTreeMap;

use crate::facts::TestResult;

type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug)]
enum Value {
    Text(String),
    Flag(bool),
}

impl Value {
    fn truthy(&self) -> bool {
        match self {
            Self::Text(text) => !text.is_empty(),
            Self::Flag(flag) => *flag,
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Flag(flag) => flag.to_string(),
        }
    }
}

/// The values a condition reads, and whether every step before it succeeded.
#[derive(Default)]
pub(crate) struct Context {
    values: BTreeMap<String, String>,
    pub(crate) job_ok: bool,
}

impl Context {
    pub(crate) fn new(values: &[(&str, &str)]) -> Self {
        Self {
            values: values
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            job_ok: true,
        }
    }

    pub(crate) fn set(&mut self, key: &str, value: &str) {
        self.values.insert(key.to_owned(), value.to_owned());
    }
}

/// The condition of a rendered step block, with the YAML quoting undone.
pub(crate) fn condition_of(step: &str) -> Fallible<String> {
    let line = step
        .lines()
        .find_map(|line| line.trim().strip_prefix("if: "))
        .ok_or("step has no if")?;
    let inner = line
        .strip_prefix("'${{ ")
        .and_then(|rest| rest.strip_suffix(" }}'"))
        .ok_or_else(|| format!("unexpected if: {line}"))?;
    Ok(inner.replace("''", "'"))
}

/// Whether a step with this condition runs: a condition without a status function
/// carries the implicit `success()`.
pub(crate) fn runs(condition: &str, context: &Context) -> Fallible<bool> {
    let status = ["success()", "failure()", "always()", "cancelled()"]
        .iter()
        .any(|function| condition.contains(function));
    Ok((status || context.job_ok) && holds(condition, context)?)
}

pub(crate) fn holds(condition: &str, context: &Context) -> Fallible<bool> {
    let mut parser = Parser {
        tokens: tokens(condition)?,
        at: 0,
        context,
    };
    let value = parser.or()?;
    if parser.at != parser.tokens.len() {
        return Err(format!("trailing tokens in {condition}").into());
    }
    Ok(value.truthy())
}

fn tokens(text: &str) -> Fallible<Vec<String>> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        let pair: String = chars[at..(at + 2).min(chars.len())].iter().collect();
        if c.is_whitespace() {
            at += 1;
        } else if c == '\'' {
            let mut literal = String::from("'");
            at += 1;
            while chars.get(at).ok_or("unterminated string")? != &'\'' {
                literal.push(chars[at]);
                at += 1;
            }
            at += 1;
            tokens.push(literal);
        } else if ["&&", "||", "==", "!="].contains(&pair.as_str()) {
            tokens.push(pair);
            at += 2;
        } else if "()!,".contains(c) {
            tokens.push(c.to_string());
            at += 1;
        } else {
            let word: String = chars[at..]
                .iter()
                .take_while(|c| c.is_alphanumeric() || "_.*-".contains(**c))
                .collect();
            if word.is_empty() {
                return Err(format!("cannot read {text} at {at}").into());
            }
            at += word.chars().count();
            tokens.push(word);
        }
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: Vec<String>,
    at: usize,
    context: &'a Context,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.at).map(String::as_str)
    }

    fn take(&mut self, wanted: &str) -> bool {
        let found = self.peek() == Some(wanted);
        self.at += usize::from(found);
        found
    }

    fn or(&mut self) -> Fallible<Value> {
        let mut left = self.and()?;
        while self.take("||") {
            let right = self.and()?;
            left = if left.truthy() { left } else { right };
        }
        Ok(left)
    }

    fn and(&mut self) -> Fallible<Value> {
        let mut left = self.equality()?;
        while self.take("&&") {
            let right = self.equality()?;
            left = if left.truthy() { right } else { left };
        }
        Ok(left)
    }

    fn equality(&mut self) -> Fallible<Value> {
        let left = self.not()?;
        for (operator, same) in [("==", true), ("!=", false)] {
            if self.take(operator) {
                let right = self.not()?;
                let equal = left.text().eq_ignore_ascii_case(&right.text());
                return Ok(Value::Flag(equal == same));
            }
        }
        Ok(left)
    }

    fn not(&mut self) -> Fallible<Value> {
        if self.take("!") {
            return Ok(Value::Flag(!self.not()?.truthy()));
        }
        self.primary()
    }

    fn primary(&mut self) -> Fallible<Value> {
        let token = self.peek().ok_or("expression ended early")?.to_owned();
        self.at += 1;
        if token == "(" {
            let inner = self.or()?;
            return if self.take(")") {
                Ok(inner)
            } else {
                Err("missing )".into())
            };
        }
        if let Some(literal) = token.strip_prefix('\'') {
            return Ok(Value::Text(literal.to_owned()));
        }
        if self.take("(") {
            let mut arguments = Vec::new();
            while !self.take(")") {
                arguments.push(self.or()?);
                self.take(",");
            }
            return self.call(&token, &arguments);
        }
        Ok(Value::Text(
            self.context.values.get(&token).cloned().unwrap_or_default(),
        ))
    }

    fn call(&self, name: &str, arguments: &[Value]) -> Fallible<Value> {
        let text = |index: usize| arguments.get(index).map(Value::text).unwrap_or_default();
        Ok(match name {
            "cancelled" => Value::Flag(false),
            "always" => Value::Flag(true),
            "success" => Value::Flag(self.context.job_ok),
            "failure" => Value::Flag(!self.context.job_ok),
            "contains" => Value::Flag(text(0).to_lowercase().contains(&text(1).to_lowercase())),
            "format" => {
                let mut formatted = text(0);
                for index in 1..arguments.len() {
                    formatted = formatted.replace(&format!("{{{}}}", index - 1), &text(index));
                }
                Value::Text(formatted)
            }
            other => return Err(format!("unknown function {other}").into()),
        })
    }
}

fn eval(condition: &str, pairs: &[(&str, &str)]) -> Fallible<bool> {
    holds(condition, &Context::new(pairs))
}

#[test]
fn the_evaluator_reads_the_expressions_the_generator_writes() -> TestResult {
    let pairs = [
        ("steps.a.outcome", "success"),
        ("github.ref", "refs/heads/main"),
    ];
    assert!(eval(
        "steps.a.outcome == 'success' && !cancelled()",
        &pairs
    )?);
    assert!(!eval("steps.a.outcome != 'success'", &pairs)?);
    assert!(eval(
        "(steps.x.outputs.y || steps.a.outcome) == 'success'",
        &pairs
    )?);
    assert!(eval(
        "github.ref == format('refs/heads/{0}', 'main')",
        &pairs
    )?);
    assert!(eval("contains(format(' {0} ', 'a b'), ' b ')", &pairs)?);
    assert!(!eval(
        "steps.a.outcome == 'success' && !(steps.a.outcome == 'success')",
        &pairs
    )?);
    Ok(())
}

#[test]
fn a_condition_without_a_status_function_needs_every_earlier_step_to_succeed() -> TestResult {
    let mut context = Context::new(&[]);
    assert!(runs("'a' == 'a'", &context)?);
    context.job_ok = false;
    assert!(!runs("'a' == 'a'", &context)?);
    assert!(runs("!cancelled() && 'a' == 'a'", &context)?);
    Ok(())
}
