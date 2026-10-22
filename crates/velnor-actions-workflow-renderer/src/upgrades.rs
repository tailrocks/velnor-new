//! Opt-in daily upgrade workflow (`upgrades.yml`, `[upgrades]`).
//!
//! Four jobs, each with the least authority it needs:
//!
//! - `prepare` (`contents: read`, no credential once repository code runs):
//!   resolves and verifies the newest stable Velnor first, then bumps it,
//!   every Mise tool (Rust last, so a Rust release that Velnor cannot verify
//!   yet rolls back alone), Cargo, Bun and Gradle catalog dependencies with
//!   `MISE_MINIMUM_RELEASE_AGE=0s`, regenerates `.github/` and uploads a
//!   filtered patch. Everything it produces is untrusted.
//! - `verify` (`contents: read`, runs no prepared byte): re-verifies the same
//!   Velnor release, regenerates `.github/` from the default branch plus that
//!   release manifest, and accepts the patch only when prepare's `.github/`
//!   is exactly that regeneration. It outputs the accepted tree.
//! - `publish` (`contents: read`): checks the patch is the verified one and
//!   applies to the accepted tree, and only then mints a GitHub App token for
//!   this repository (workflows permission only when a workflow changes),
//!   fast-forwards `velnor/upgrades` (never a force push) and opens or
//!   updates the one upgrade pull request.
//! - `freshness` (`contents: read`, `pull-requests: read`): fails while any
//!   pin lags the latest stable release or the pull request has been open
//!   longer than `stuck_days`. It runs only on this schedule, so it is never
//!   a pull-request check.

use velnor_actions_contract::config::{UPGRADES_WORKFLOW_PATH, UpgradesConfig};

use crate::{MiseSetup, RenderError, guard, marker, render::RenderedFile, steps, yaml::Yaml};

/// Display name of the upgrade workflow.
pub const UPGRADES_WORKFLOW_NAME: &str = "Velnor Upgrades";
/// Branch the upgrade pull request lives on.
pub const UPGRADES_BRANCH: &str = "velnor/upgrades";
/// Concurrency group: scheduled runs serialize and never cancel.
pub const UPGRADES_CONCURRENCY_GROUP: &str = "velnor-upgrades";
/// Patch artifact handed from `prepare` to `publish`.
pub const UPGRADES_ARTIFACT: &str = "velnor-upgrade";
/// Exact `cargo-edit` release used for `cargo upgrade` (no release binaries
/// exist, so Mise builds it from crates.io; maintenance job only).
/// Source: `https://crates.io/api/v1/crates/cargo-edit`; checked 2026-10-22.
pub const CARGO_EDIT_VERSION: &str = "0.13.13";

#[path = "upgrades_jobs.rs"]
mod jobs;
#[path = "upgrades_scripts.rs"]
mod scripts;
use jobs::{env, gate_job, prepare_job, publish_job, verify_job};
#[cfg(test)]
use scripts::{FILTER, PUBLISH, RELEASE, publish_check_script, verify_script};
const CATALOG_BUMPER: &str = include_str!("../../../scripts/upgrades/bump-gradle-catalog.py");
const WORK_DIR: &str = "${{ runner.temp }}/velnor-upgrades";
const ON_DEFAULT_BRANCH: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";

/// Validated upgrade-workflow inputs.
#[derive(Debug, Clone)]
pub struct UpgradesSpec {
    /// The enabled `[upgrades]` table.
    pub config: UpgradesConfig,
    /// Single literal versioned Ubuntu label.
    pub runs_on: String,
    /// Pinned `actions/checkout` ref.
    pub checkout_uses: String,
    /// Compiled, digest-verified Mise setup action.
    pub mise_setup: MiseSetup,
    /// Exact Velnor-pinned GitHub CLI version (never project-selected).
    pub gh_version: String,
    /// Exact generator version for the marker.
    pub generator_version: String,
    /// Exact Velnor-pinned tools `velnor-actions generate` executes
    /// (`name@version`), installed before each regeneration.
    pub generate_tools: Vec<String>,
    /// One-line `sh` script that checks the installed jactionlint executable
    /// against the catalog digest; it runs right after the install.
    pub jactionlint_verify: String,
}

impl UpgradesSpec {
    /// Validate every spec scalar before rendering.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] describing the first invalid scalar.
    pub fn validate(&self) -> Result<(), RenderError> {
        self.config
            .validate("upgrades")
            .map_err(RenderError::Contract)?;
        if !self.config.enabled {
            return Err(RenderError::InvalidWorkflow("upgrades_disabled".to_owned()));
        }
        marker::validate_version(&self.generator_version)?;
        guard::validate_runs_on(&self.runs_on)?;
        steps::validate_uses(&self.checkout_uses)?;
        self.mise_setup.validate()?;
        if !crate::commands::is_exact_or_project_version(&self.gh_version)
            || self.gh_version.starts_with("${")
        {
            return Err(RenderError::BadCommand("bad_gh_version".to_owned()));
        }
        if self.generate_tools.is_empty()
            || !self.generate_tools.iter().all(|spec| exact_tool(spec))
        {
            return Err(RenderError::BadCommand("bad_generate_tools".to_owned()));
        }
        if self.jactionlint_verify.trim().is_empty()
            || self.jactionlint_verify.contains('\n')
            || self.jactionlint_verify.contains("${{")
        {
            return Err(RenderError::BadCommand("bad_jactionlint_verify".to_owned()));
        }
        Ok(())
    }
}

/// `name@exact-version` with only portable characters and no expression.
fn exact_tool(spec: &str) -> bool {
    spec.split_once('@').is_some_and(|(name, version)| {
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && crate::commands::is_exact_or_project_version(version)
            && !version.starts_with("${")
    })
}

/// Render `upgrades.yml` from a validated spec.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid spec scalars or steps.
pub fn render_upgrades_workflow(spec: &UpgradesSpec) -> Result<RenderedFile, RenderError> {
    spec.validate()?;
    let document = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(UPGRADES_WORKFLOW_NAME)),
        (
            "on".to_owned(),
            Yaml::Map(vec![
                (
                    "schedule".to_owned(),
                    Yaml::Seq(vec![Yaml::Map(vec![(
                        "cron".to_owned(),
                        Yaml::str(spec.config.schedule.clone()),
                    )])]),
                ),
                ("workflow_dispatch".to_owned(), Yaml::Map(Vec::new())),
            ]),
        ),
        ("permissions".to_owned(), env(&[("contents", "read")])),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                ("group".to_owned(), Yaml::str(UPGRADES_CONCURRENCY_GROUP)),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![
                ("prepare".to_owned(), prepare_job(spec)?),
                ("verify".to_owned(), verify_job(spec)?),
                ("publish".to_owned(), publish_job(spec)?),
                ("freshness".to_owned(), gate_job(spec)?),
            ]),
        ),
    ]);
    let text = crate::render::fallback::render_checked_workflow(
        UPGRADES_WORKFLOW_PATH,
        &document,
        &spec.generator_version,
    )?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(RenderedFile {
        path: UPGRADES_WORKFLOW_PATH.to_owned(),
        bytes: text,
    })
}

#[cfg(test)]
#[path = "upgrades_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "upgrades_exec_fakes.rs"]
mod exec_fakes;

#[cfg(test)]
#[path = "upgrades_exec_test_support.rs"]
mod exec_support;

#[cfg(test)]
#[path = "upgrades_filter_exec_tests.rs"]
mod filter_exec_tests;

#[cfg(test)]
#[path = "upgrades_verify_exec_tests.rs"]
mod verify_exec_tests;
