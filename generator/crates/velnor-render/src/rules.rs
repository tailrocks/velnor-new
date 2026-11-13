// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

pub const FETCH_HELPER_RUN: &str = "\
curl -fsSL --create-dirs -o \"$RUNNER_TEMP/vbin/velnor-ci\" \"$URL\"
echo \"$SHA  $RUNNER_TEMP/vbin/velnor-ci\" > \"$RUNNER_TEMP/velnor-ci.sha\"
shasum -a 256 -c \"$RUNNER_TEMP/velnor-ci.sha\"
chmod +x \"$RUNNER_TEMP/vbin/velnor-ci\"
echo \"$RUNNER_TEMP/vbin\" >> \"$GITHUB_PATH\"";

pub const ALLOWED_ACTIONS: [&str; 6] = [
    "actions/checkout",
    "actions/cache/restore",
    "actions/cache/save",
    "actions/upload-artifact",
    "actions/download-artifact",
    "jdx/mise-action",
];

pub const MAX_RUN_CHARACTERS: usize = 2_000;

const FORBIDDEN_WORDS: [&str; 4] = ["if", "for", "while", "case"];
const FORBIDDEN_SYMBOLS: [&str; 9] = ["|", "&", "&&", "||", ";", "$(", "`", "<(", ">("];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Expr(String),
    Int(u64),
    Bool(bool),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedStep {
    pub name: String,
    pub uses: Option<String>,
    pub with: Vec<(String, Value)>,
    pub working_directory: Option<String>,
    pub env: Vec<(String, Value)>,
    pub shell: Option<String>,
    pub run: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedJob {
    pub id: String,
    pub runs_on: String,
    pub timeout_minutes: u16,
    pub steps: Vec<RenderedStep>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedWorkflow {
    pub jobs: Vec<RenderedJob>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rule {
    MultiLineRun,
    ForbiddenSyntax(&'static str),
    RunTooLong,
    ExpressionInRun,
    ShellKey,
    NotExactlyOneOfRunAndUses,
    ActionNotAllowed(String),
    ActionNotPinned(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleViolation {
    pub job: String,
    pub step: usize,
    pub rule: Rule,
}

impl fmt::Display for RuleViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "job {} step {}: ", self.job, self.step + 1)?;
        match &self.rule {
            Rule::MultiLineRun => f.write_str("a run value has more than one line"),
            Rule::ForbiddenSyntax(token) => write!(f, "a run value contains `{token}`"),
            Rule::RunTooLong => write!(
                f,
                "a run value is longer than {MAX_RUN_CHARACTERS} characters"
            ),
            Rule::ExpressionInRun => {
                f.write_str("a run value contains an expression; pass it through env")
            }
            Rule::NotExactlyOneOfRunAndUses => {
                f.write_str("a step needs exactly one of run and uses")
            }
            Rule::ShellKey => f.write_str("a step sets shell"),
            Rule::ActionNotAllowed(action) => {
                write!(f, "action {action} is not on the allowed list")
            }
            Rule::ActionNotPinned(action) => {
                write!(f, "action {action} is not pinned to a full commit SHA")
            }
        }
    }
}

pub fn check_rules(workflow: &RenderedWorkflow) -> Result<(), Vec<RuleViolation>> {
    let violations: Vec<RuleViolation> = workflow
        .jobs
        .iter()
        .flat_map(|job| {
            job.steps.iter().enumerate().flat_map(move |(index, step)| {
                step_rules(step).into_iter().map(move |rule| RuleViolation {
                    job: job.id.clone(),
                    step: index,
                    rule,
                })
            })
        })
        .collect();
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

fn step_rules(step: &RenderedStep) -> Vec<Rule> {
    let mut rules = Vec::new();
    if let Some(run) = step.run.as_deref().filter(|run| *run != FETCH_HELPER_RUN) {
        run_rules(run, &mut rules);
    }
    if step.run.is_some() == step.uses.is_some() {
        rules.push(Rule::NotExactlyOneOfRunAndUses);
    }
    if step.shell.is_some() {
        rules.push(Rule::ShellKey);
    }
    if let Some(uses) = &step.uses {
        action_rules(uses, &mut rules);
    }
    rules
}

fn run_rules(run: &str, rules: &mut Vec<Rule>) {
    if run.contains('\n') {
        rules.push(Rule::MultiLineRun);
    }
    for word in FORBIDDEN_WORDS {
        if has_word(run, word) {
            rules.push(Rule::ForbiddenSyntax(word));
        }
    }
    for symbol in FORBIDDEN_SYMBOLS {
        if run.contains(symbol) {
            rules.push(Rule::ForbiddenSyntax(symbol));
        }
    }
    if run.chars().count() > MAX_RUN_CHARACTERS {
        rules.push(Rule::RunTooLong);
    }
    if run.contains("${{") {
        rules.push(Rule::ExpressionInRun);
    }
}

fn has_word(run: &str, word: &str) -> bool {
    run.match_indices(word).any(|(start, _)| {
        let before = run[..start].chars().next_back();
        let after = run[start + word.len()..].chars().next();
        before.is_none_or(char::is_whitespace) && after.is_some_and(char::is_whitespace)
    })
}

fn action_rules(uses: &str, rules: &mut Vec<Rule>) {
    let (action, reference) = uses.split_once('@').unwrap_or((uses, ""));
    if !ALLOWED_ACTIONS.contains(&action) {
        rules.push(Rule::ActionNotAllowed(action.to_owned()));
    }
    let pinned = reference.len() == 40
        && reference
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !pinned {
        rules.push(Rule::ActionNotPinned(uses.to_owned()));
    }
}
