//! Declared Bun suites and their Playwright matrix (hard decision 6 amendment).
//!
//! A suite groups some of a member's scripts into a job beside the member
//! job. Like the rest of `[stacks.bun]` the tables are strict and carry only
//! script names, browser names, project names and bounded numbers.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::BunBrowser;
use crate::config::workflow_flags::{default_true, is_true};
use crate::errors::ContractError;
use crate::workflow::timeout::JobTimeout;

/// Suite ids Velnor reserves for the jobs it derives from a suite.
pub const RESERVED_BUN_SUITE_IDS: [&str; 2] = ["assets", "report"];

/// One declared suite (`[stacks.bun.members."<name>".suites.<id>]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BunSuiteConfig {
    /// Member scripts this suite runs as parallel steps; they leave the member job.
    pub scripts: Vec<String>,
    /// Playwright browsers this suite's job installs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub browsers: Vec<BunBrowser>,
    /// Timeout of this suite's job (each leg when sharded), `1..=360`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_minutes: Option<u32>,
    /// Whether the suite job joins Required (default `true`).
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub required: bool,
    /// Present: the suite's one Playwright script runs as a static project
    /// and shard matrix of legs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playwright: Option<BunSuitePlaywright>,
}

/// Largest number of matrix legs one Playwright suite may emit (shared with
/// Rust suite partitions).
pub const MAX_BUN_SUITE_LEGS: u16 = crate::MAX_SUITE_PARTITIONS;

/// Playwright matrix of one suite (`[...suites.<id>.playwright]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BunSuitePlaywright {
    /// Playwright project names, one leg group each (sorted, unique); empty
    /// shards over every default project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
    /// Shards per project (or of the whole run without projects), `1..=64`.
    #[serde(default = "one_shard", skip_serializing_if = "is_one_shard")]
    pub shards: u16,
    /// Optional `strategy.max-parallel`, `1..=legs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<u16>,
    /// Append `--fail-on-flaky-tests`: a test that passes only on retry fails its leg.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fail_on_flaky: bool,
}

const fn one_shard() -> u16 {
    1
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&u16) -> bool"
)]
const fn is_one_shard(shards: &u16) -> bool {
    *shards == 1
}

impl BunSuitePlaywright {
    /// Number of matrix legs: projects (at least one group) times shards.
    #[must_use]
    pub fn legs(&self) -> usize {
        self.projects.len().max(1) * usize::from(self.shards)
    }
}

/// True for a Playwright project name that stays a plain YAML and env literal.
#[must_use]
pub fn is_valid_project_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

/// True for a suite id: `[a-z][a-z0-9-]{0,31}` without a trailing `-` or `--`.
#[must_use]
pub fn is_valid_suite_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.ends_with('-')
        && !id.contains("--")
}

pub(super) fn validate_suites(
    suites: &BTreeMap<String, BunSuiteConfig>,
    file: &str,
) -> Result<(), ContractError> {
    const KEY: &str = "stacks.bun.members.suites";
    let mut claimed = BTreeSet::new();
    for (id, suite) in suites {
        if !is_valid_suite_id(id) {
            return Err(ContractError::config(
                file,
                KEY,
                format!("bad_suite_id:{id}"),
            ));
        }
        if RESERVED_BUN_SUITE_IDS.contains(&id.as_str()) {
            return Err(ContractError::config(
                file,
                KEY,
                format!("reserved_suite_id:{id}"),
            ));
        }
        if suite.scripts.is_empty() {
            return Err(ContractError::config(
                file,
                KEY,
                format!("empty_suite_scripts:{id}"),
            ));
        }
        super::validate_scripts(&suite.scripts, "stacks.bun.members.suites.scripts", file)?;
        for script in &suite.scripts {
            if !claimed.insert(script.as_str()) {
                return Err(ContractError::config(
                    file,
                    KEY,
                    format!("suite_script_claimed_twice:{script}"),
                ));
            }
        }
        super::validate_browsers(&suite.browsers, file)?;
        if let Some(minutes) = suite.timeout_minutes {
            JobTimeout::check_config(minutes, file, "stacks.bun.members.suites.timeout_minutes")?;
        }
        if let Some(playwright) = &suite.playwright {
            validate_playwright(id, suite, playwright, file)?;
        }
    }
    Ok(())
}

fn validate_playwright(
    id: &str,
    suite: &BunSuiteConfig,
    playwright: &BunSuitePlaywright,
    file: &str,
) -> Result<(), ContractError> {
    const KEY: &str = "stacks.bun.members.suites.playwright";
    let fail = |problem: String| Err(ContractError::config(file, KEY, problem));
    if suite.scripts.len() != 1 {
        return fail(format!("playwright_suite_one_script:{id}"));
    }
    if suite.browsers.is_empty() {
        return fail(format!("playwright_suite_without_browsers:{id}"));
    }
    if !(1..=MAX_BUN_SUITE_LEGS).contains(&playwright.shards) {
        return fail(format!(
            "bad_shards:{}:expected_1..={MAX_BUN_SUITE_LEGS}",
            playwright.shards
        ));
    }
    if let Some(bad) = playwright
        .projects
        .iter()
        .find(|name| !is_valid_project_name(name))
    {
        return fail(format!("bad_project_name:{bad}"));
    }
    if playwright
        .projects
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        return fail("projects_must_be_sorted_and_unique".to_owned());
    }
    let legs = playwright.legs();
    if legs > usize::from(MAX_BUN_SUITE_LEGS) {
        return fail(format!(
            "too_many_legs:{legs}:expected_..={MAX_BUN_SUITE_LEGS}"
        ));
    }
    if playwright
        .max_parallel
        .is_some_and(|cap| cap == 0 || usize::from(cap) > legs)
    {
        return fail("bad_max_parallel".to_owned());
    }
    Ok(())
}
