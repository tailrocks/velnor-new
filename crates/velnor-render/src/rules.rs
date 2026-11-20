// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use crate::pinned_actions::{
    ACTIONS_CACHE, ATTEST_BUILD_PROVENANCE, DOCKER_BUILD_PUSH, DOCKER_LOGIN, DOCKER_SETUP_BUILDX,
    GITHUB_SCRIPT, MR_BOXINGTON,
};

mod check;
mod model;

pub use check::check_rules;
pub use model::{
    RenderedConcurrency, RenderedContainer, RenderedJob, RenderedMatrix, RenderedMatrixSource,
    RenderedStep, RenderedWorkflow, Value,
};

pub const SETUP_VELNOR_ACTION: &str = "tailrocks/velnor-new/actions/setup-velnor";

pub const APP_TOKEN_ACTION: &str = "actions/create-github-app-token";

pub const ALLOWED_ACTIONS: [&str; 16] = [
    "actions/checkout",
    APP_TOKEN_ACTION,
    "actions/cache/restore",
    "actions/cache/save",
    "actions/upload-artifact",
    "actions/download-artifact",
    MISE_ACTION,
    SETUP_VELNOR_ACTION,
    MBX_ACTION,
    ATTEST_BUILD_PROVENANCE.action,
    DOCKER_LOGIN.action,
    DOCKER_SETUP_BUILDX.action,
    DOCKER_BUILD_PUSH.action,
    GITHUB_SCRIPT.action,
    MR_BOXINGTON.action,
    ACTIONS_CACHE.action,
];

pub(crate) const MBX_ACTION: &str = "jdx/mr-boxington-action";
pub(crate) const MISE_ACTION: &str = "jdx/mise-action";

pub const MAX_RUN_CHARACTERS: usize = 2_000;
pub const MAX_WORKFLOW_BYTES: usize = 500_000;
pub const MAX_CACHE_KEY_CHARACTERS: usize = 512;
pub const MAX_ENV_CHARACTERS: usize = 100_000;

const FORBIDDEN_WORDS: [&str; 5] = ["if", "for", "while", "case", "coproc"];
const FORBIDDEN_SYMBOLS: [&str; 9] = ["|", "&", "&&", "||", ";", "$(", "`", "<(", ">("];

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
    ActionPinMismatch {
        action: String,
        commit: &'static str,
    },
    LiteralCredential {
        action: String,
        input: String,
    },
    MiseActionInTokenJob,
    MiseReadsRepositoryConfig,
    UnreadableCommandInTokenJob,
    CombinedCacheNotLastStep,
    TokenJobRunnerNotX64(String),
    WorkflowTooLarge(usize),
    CacheKeyTooLong(usize),
    OutputNameCollision(String),
    DistFileCollision(String),
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
        if let Some(message) = crate::rule_messages::whole_workflow(&self.rule, &self.job) {
            return f.write_str(&message);
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
            Rule::ActionPinMismatch { action, commit } => write!(
                f,
                "action {action} is accepted only at commit {commit}; use that commit"
            ),
            Rule::LiteralCredential { action, input } => write!(
                f,
                "action {action} input {input} is a literal; pass it through a secrets or env expression"
            ),
            Rule::MiseActionInTokenJob => f.write_str(
                "a job that holds the release App token uses jdx/mise-action, which reads the repository's mise config",
            ),
            Rule::MiseReadsRepositoryConfig => f.write_str(
                "a job that holds the release App token runs mise without --no-config --no-env --no-hooks",
            ),
            Rule::UnreadableCommandInTokenJob => f.write_str(
                "a job that holds the release App token runs a command that cannot be read, which may run mise with the repository config",
            ),
            Rule::CombinedCacheNotLastStep => f.write_str(crate::combined_cache::MESSAGE),
            Rule::WorkflowTooLarge(_)
            | Rule::OutputNameCollision(_)
            | Rule::DistFileCollision(_) => Ok(()),
            Rule::CacheKeyTooLong(length) => write!(
                f,
                "a cache key can be {length} characters long at run time, and actions/cache allows at most {MAX_CACHE_KEY_CHARACTERS}; shorten the job id or the root directory name"
            ),
            Rule::EnvTooLong(length) => write!(
                f,
                "an env value can expand to {length} UTF-8 bytes, and the limit is {MAX_ENV_CHARACTERS}; the required gate's needs JSON includes every dependency's outputs, so reduce the Cargo roots or package outputs"
            ),
            Rule::TokenJobRunnerNotX64(runner) => write!(
                f,
                "runner {runner:?} is not a linux x64 runner, and the pinned mise digest is for linux x64 only; set rust-ci runner to ubuntu-latest or another ubuntu x64 label for releases"
            ),
        }
    }
}
