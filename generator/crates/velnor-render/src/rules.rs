// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use crate::key_length::longest_cache_key;

pub const SETUP_VELNOR_ACTION: &str = "tailrocks/velnor-new/actions/setup-velnor";

pub const APP_TOKEN_ACTION: &str = "actions/create-github-app-token";

pub const HELPER_FROM_ARTIFACT_RUN: &str = "\
chmod +x \"$RUNNER_TEMP/vbin/velnor-ci\"
echo \"$RUNNER_TEMP/vbin\" >> \"$GITHUB_PATH\"";

pub const ALLOWED_ACTIONS: [&str; 8] = [
    "actions/checkout",
    APP_TOKEN_ACTION,
    "actions/cache/restore",
    "actions/cache/save",
    "actions/upload-artifact",
    "actions/download-artifact",
    MISE_ACTION,
    SETUP_VELNOR_ACTION,
];

pub(crate) const MISE_ACTION: &str = "jdx/mise-action";

pub const MAX_RUN_CHARACTERS: usize = 2_000;
pub(crate) const MAX_WORKFLOW_BYTES: usize = 500_000;
pub const MAX_CACHE_KEY_CHARACTERS: usize = 512;
pub const MAX_ENV_CHARACTERS: usize = 100_000;

const FORBIDDEN_WORDS: [&str; 5] = ["if", "for", "while", "case", "coproc"];
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
    pub id: Option<String>,
    pub name: String,
    pub condition: Option<String>,
    pub continue_on_error: bool,
    pub uses: Option<String>,
    pub uses_comment: Option<String>,
    pub with: Vec<(String, Value)>,
    pub working_directory: Option<String>,
    pub env: Vec<(String, Value)>,
    pub shell: Option<String>,
    pub run: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedConcurrency {
    pub group: String,
    pub cancel_in_progress: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedJob {
    pub id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    pub needs: Vec<String>,
    pub condition: Option<String>,
    pub environment: Option<String>,
    pub permissions: Vec<(String, String)>,
    pub no_permissions: bool,
    pub concurrency: Option<RenderedConcurrency>,
    pub outputs: Vec<(String, String)>,
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
    MiseActionInTokenJob,
    MiseReadsRepositoryConfig,
    TokenJobRunnerNotX64(String),
    WorkflowTooLarge(usize),
    CacheKeyTooLong(usize),
    OutputNameCollision(String),
    EnvTooLong(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleViolation {
    pub job: String,
    pub step: usize,
    pub rule: Rule,
}

impl fmt::Display for RuleViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Rule::WorkflowTooLarge(bytes) = &self.rule {
            return write!(
                f,
                "the workflow is {bytes} bytes, and GitHub does not start a workflow file above {MAX_WORKFLOW_BYTES} bytes; release fewer binaries from one repository"
            );
        }
        if let Rule::OutputNameCollision(name) = &self.rule {
            return write!(
                f,
                "the tag job would define the output {name} twice: it is both a released package and the commit output of another package; rename the package"
            );
        }
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
            Rule::MiseActionInTokenJob => f.write_str(
                "a job that holds the release App token uses jdx/mise-action, which reads the repository's mise config",
            ),
            Rule::MiseReadsRepositoryConfig => f.write_str(
                "a job that holds the release App token runs mise without --no-config --no-env --no-hooks",
            ),
            Rule::WorkflowTooLarge(_) | Rule::OutputNameCollision(_) => Ok(()),
            Rule::CacheKeyTooLong(length) => write!(
                f,
                "a cache key can be {length} characters long at run time, and actions/cache allows at most {MAX_CACHE_KEY_CHARACTERS}; shorten the job id or the root directory name"
            ),
            Rule::EnvTooLong(length) => write!(
                f,
                "an env value is {length} characters long, and the limit is {MAX_ENV_CHARACTERS}; the gate spec grows with the number of jobs, so split the repository's Cargo roots"
            ),
            Rule::TokenJobRunnerNotX64(runner) => write!(
                f,
                "runner {runner:?} is not a linux x64 runner, and the pinned mise digest is for linux x64 only; set rust-ci runner to ubuntu-latest or another ubuntu x64 label for releases"
            ),
        }
    }
}

pub fn check_rules(workflow: &RenderedWorkflow) -> Result<(), Vec<RuleViolation>> {
    let violations: Vec<RuleViolation> = workflow
        .jobs
        .iter()
        .flat_map(|job| {
            let holds_token = holds_app_token(job);
            let step_violations = job.steps.iter().enumerate().flat_map(move |(index, step)| {
                let mut rules = step_rules(step);
                if holds_token {
                    token_step_rules(step, &mut rules);
                }
                rules.into_iter().map(move |rule| RuleViolation {
                    job: job.id.clone(),
                    step: index,
                    rule,
                })
            });
            let runner = (holds_token && !is_linux_x64(&job.runs_on)).then(|| RuleViolation {
                job: job.id.clone(),
                step: 0,
                rule: Rule::TokenJobRunnerNotX64(job.runs_on.clone()),
            });
            step_violations.chain(runner)
        })
        .collect();
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

fn holds_app_token(job: &RenderedJob) -> bool {
    job.steps.iter().any(|step| {
        step.uses
            .as_deref()
            .is_some_and(|uses| uses.split('@').next() == Some(APP_TOKEN_ACTION))
    })
}

fn is_linux_x64(runner: &str) -> bool {
    runner == "ubuntu-latest" || (runner.starts_with("ubuntu-") && !runner.contains("arm"))
}

fn token_step_rules(step: &RenderedStep, rules: &mut Vec<Rule>) {
    if step
        .uses
        .as_deref()
        .is_some_and(|uses| uses.split('@').next() == Some(MISE_ACTION))
    {
        rules.push(Rule::MiseActionInTokenJob);
    }
    let program = step
        .run
        .as_deref()
        .and_then(|run| run.split_whitespace().next())
        .map(|word| word.trim_matches('"'));
    if program.is_some_and(|word| word == "mise" || word.ends_with("/mise"))
        && !step
            .run
            .as_deref()
            .is_some_and(|run| run.contains(" --no-config --no-env --no-hooks "))
    {
        rules.push(Rule::MiseReadsRepositoryConfig);
    }
}

fn step_rules(step: &RenderedStep) -> Vec<Rule> {
    let mut rules = Vec::new();
    if let Some(run) = step
        .run
        .as_deref()
        .filter(|run| *run != HELPER_FROM_ARTIFACT_RUN)
    {
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
    if let Some(length) = longest_cache_key(step)
        && length > MAX_CACHE_KEY_CHARACTERS
    {
        rules.push(Rule::CacheKeyTooLong(length));
    }
    if let Some(length) = longest_env(step)
        && length > MAX_ENV_CHARACTERS
    {
        rules.push(Rule::EnvTooLong(length));
    }
    rules
}

fn longest_env(step: &RenderedStep) -> Option<usize> {
    step.env
        .iter()
        .filter_map(|(_, value)| match value {
            Value::Text(text) => Some(text.chars().count()),
            _ => None,
        })
        .max()
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
