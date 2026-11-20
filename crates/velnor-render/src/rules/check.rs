// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    ALLOWED_ACTIONS, APP_TOKEN_ACTION, FORBIDDEN_SYMBOLS, FORBIDDEN_WORDS,
    MAX_CACHE_KEY_CHARACTERS, MAX_ENV_CHARACTERS, MAX_RUN_CHARACTERS, MISE_ACTION, RenderedJob,
    RenderedStep, RenderedWorkflow, Rule, RuleViolation, Value,
};
use crate::key_length::longest_cache_key;
use crate::pinned_actions::{credential_rules, pin_rules};

pub fn check_rules(workflow: &RenderedWorkflow) -> Result<(), Vec<RuleViolation>> {
    let violations: Vec<RuleViolation> = workflow
        .jobs
        .iter()
        .flat_map(|job| {
            let holds_token = holds_app_token(job);
            let last = job.steps.len().saturating_sub(1);
            let step_violations = job.steps.iter().enumerate().flat_map(move |(index, step)| {
                let mut rules = step_rules(step);
                if crate::combined_cache::before_last(step, index, last) {
                    rules.push(Rule::CombinedCacheNotLastStep);
                }
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
        ![
            crate::private_steps::TOKEN_STEP,
            crate::private_steps::release_asset_steps::TOKEN_STEP,
        ]
        .contains(&step.id.as_deref().unwrap_or_default())
            && step
                .uses
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
    if let Some(run) = step.run.as_deref() {
        rules.extend(crate::token_commands::rules(run));
    }
}

fn step_rules(step: &RenderedStep) -> Vec<Rule> {
    let mut rules = Vec::new();
    if let Some(run) = step.run.as_deref() {
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
    credential_rules(step, &mut rules);
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
            Value::Text(text) => Some(text.len()),
            Value::BoundedExpr { max_bytes, .. } => Some(*max_bytes),
            _ => None,
        })
        .max()
}

fn run_rules(run: &str, rules: &mut Vec<Rule>) {
    if run.contains('\n') {
        rules.push(Rule::MultiLineRun);
    }
    let command = run
        .strip_prefix(crate::helper_integrity::INVOCATION_PREFIX)
        .unwrap_or(run);
    for word in FORBIDDEN_WORDS {
        if has_word(run, word) {
            rules.push(Rule::ForbiddenSyntax(word));
        }
    }
    for symbol in FORBIDDEN_SYMBOLS {
        if command.contains(symbol) {
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
    if uses.starts_with("./") {
        if velnor_workflows::LocalAction::parse(uses).is_none() {
            rules.push(Rule::ActionNotAllowed(uses.to_owned()));
        }
        return;
    }
    let (action, reference) = uses.split_once('@').unwrap_or((uses, ""));
    if !ALLOWED_ACTIONS.contains(&action) {
        rules.push(Rule::ActionNotAllowed(action.to_owned()));
    }
    let pinned = reference.len() == 40
        && reference
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if pinned {
        pin_rules(action, reference, rules);
    } else {
        rules.push(Rule::ActionNotPinned(uses.to_owned()));
    }
}
