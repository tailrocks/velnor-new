//! Scheduled upstream-freshness workflow rendering (P12-4 emission half).
//!
//! Third render entrypoint: [`render_freshness_workflow`] turns a
//! validated [`FreshnessSpec`] into `freshness.yml` — a schedule-only,
//! read-only probe that runs the bounded
//! `scripts/check-freshness.sh --check-upstream` over the Velnor-owned
//! inventory. The contract half ([`ScheduleTrigger`],
//! [`FRESHNESS_CRON_WEEKLY`], [`FRESHNESS_WORKFLOW_PATH`]) lives in
//! `velnor-actions-contract`; the main CI renderer cannot serve here
//! because it mandates PR/push/merge-group triggers plus plan, task,
//! and final closures.
//!
//! [`ScheduleTrigger`]: velnor_actions_contract::ScheduleTrigger
//! [`FRESHNESS_CRON_WEEKLY`]: velnor_actions_contract::FRESHNESS_CRON_WEEKLY
//! [`FRESHNESS_WORKFLOW_PATH`]: velnor_actions_contract::FRESHNESS_WORKFLOW_PATH

use std::collections::BTreeMap;

use velnor_actions_contract::{FRESHNESS_WORKFLOW_PATH, ScheduleTrigger, Step, StepRole};

use crate::{
    MiseSetup, RenderError, guard, marker, mise_setup_step,
    render::RenderedFile,
    steps::{self, action_step, shell_step},
    yaml::Yaml,
};

/// Display name of the freshness workflow.
pub const FRESHNESS_WORKFLOW_NAME: &str = "Upstream Freshness";
/// Stable freshness job ID.
pub const FRESHNESS_JOB_ID: &str = "freshness";
/// Display name of the freshness job.
pub const FRESHNESS_JOB_NAME: &str = "Freshness";
/// The bounded read-only probe this workflow runs, fixed.
pub const FRESHNESS_SCRIPT: &str = "scripts/check-freshness.sh";
/// Probe flag: refetch upstream evidence, write nothing.
pub const FRESHNESS_PROBE_FLAG: &str = "--check-upstream";
/// Freshness concurrency group (scheduled runs serialize, never cancel).
pub const FRESHNESS_CONCURRENCY_GROUP: &str = "freshness";
/// Freshness job timeout: every row may hit its 10 s probe cap.
pub const FRESHNESS_TIMEOUT_MINUTES: i64 = 10;

/// Validated freshness-workflow inputs.
#[derive(Debug, Clone)]
pub struct FreshnessSpec {
    /// Cron schedule (the reviewed weekly trigger).
    pub schedule: ScheduleTrigger,
    /// Single literal versioned Ubuntu label.
    pub runs_on: String,
    /// Pinned `actions/checkout` ref.
    pub checkout_uses: String,
    /// Compiled, digest-verified Mise setup action.
    pub mise_setup: MiseSetup,
    /// Exact Rust toolchain installed and used by the freshness script.
    pub rust_version: String,
    /// Exact generator version for the marker.
    pub generator_version: String,
}

impl FreshnessSpec {
    /// Validate every spec scalar before rendering.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] describing the first invalid scalar.
    pub fn validate(&self) -> Result<(), RenderError> {
        self.schedule.validate().map_err(RenderError::Contract)?;
        marker::validate_version(&self.generator_version)?;
        guard::validate_runs_on(&self.runs_on)?;
        steps::validate_uses(&self.checkout_uses)?;
        self.mise_setup.validate()?;
        if !valid_rust_version(&self.rust_version) {
            return Err(RenderError::BadCommand("bad_rust_version".to_owned()));
        }
        if !self.checkout_uses.starts_with("actions/checkout@") {
            return Err(RenderError::BadActionRef(format!(
                "not_checkout:{}",
                self.checkout_uses
            )));
        }
        Ok(())
    }
}

/// Accept one exact numeric Rust toolchain version, with no shell syntax.
fn valid_rust_version(value: &str) -> bool {
    let mut components = value.split('.');
    let Some(major) = components.next() else {
        return false;
    };
    let Some(minor) = components.next() else {
        return false;
    };
    let Some(patch) = components.next() else {
        return false;
    };
    components.next().is_none()
        && [major, minor, patch]
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Render the freshness workflow document from a validated spec.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid spec scalars or steps.
pub fn render_freshness_workflow(spec: &FreshnessSpec) -> Result<RenderedFile, RenderError> {
    spec.validate()?;
    let mut checkout = action_step(
        "Checkout",
        &spec.checkout_uses,
        BTreeMap::from([
            ("persist-credentials".to_owned(), "false".to_owned()),
            ("fetch-depth".to_owned(), "1".to_owned()),
        ]),
    )?;
    checkout.role = Some(StepRole::Checkout);
    let setup_mise = mise_setup_step(&spec.mise_setup)?;
    let install_rust = shell_step(
        "Install Rust toolchain",
        vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "--no-env".to_owned(),
            "--no-hooks".to_owned(),
            "install".to_owned(),
            format!("rust@{}", spec.rust_version),
        ],
        BTreeMap::new(),
    )?;
    let probe = shell_step(
        "Check upstream freshness",
        vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "--no-env".to_owned(),
            "--no-hooks".to_owned(),
            "exec".to_owned(),
            format!("rust@{}", spec.rust_version),
            "--".to_owned(),
            "bash".to_owned(),
            FRESHNESS_SCRIPT.to_owned(),
            FRESHNESS_PROBE_FLAG.to_owned(),
        ],
        BTreeMap::new(),
    )?;
    let document = freshness_document(spec, &checkout, &setup_mise, &install_rust, &probe)?;
    let text = crate::render::fallback::render_checked_workflow(
        FRESHNESS_WORKFLOW_PATH,
        &document,
        &spec.generator_version,
    )?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(RenderedFile {
        path: FRESHNESS_WORKFLOW_PATH.to_owned(),
        bytes: text,
    })
}

/// Build the freshness document: name, schedule triggers, perms, job.
fn freshness_document(
    spec: &FreshnessSpec,
    checkout: &Step,
    setup_mise: &Step,
    install_rust: &Step,
    probe: &Step,
) -> Result<Yaml, RenderError> {
    velnor_actions_contract::workflow::step_identity::validate_step_sequence(
        &[
            checkout.clone(),
            setup_mise.clone(),
            install_rust.clone(),
            probe.clone(),
        ],
        FRESHNESS_JOB_ID,
    )
    .map_err(RenderError::Contract)?;
    let crons: Vec<Yaml> = spec
        .schedule
        .cron
        .iter()
        .map(|cron| Yaml::Map(vec![("cron".to_owned(), Yaml::str(cron.clone()))]))
        .collect();
    let job = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(FRESHNESS_JOB_NAME)),
        ("runs-on".to_owned(), Yaml::str(spec.runs_on.clone())),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(FRESHNESS_TIMEOUT_MINUTES),
        ),
        ("permissions".to_owned(), read_only_permissions()),
        (
            "steps".to_owned(),
            Yaml::Seq(vec![
                crate::steps_plain::plain_step_to_yaml(checkout)?,
                crate::steps_plain::plain_step_to_yaml(setup_mise)?,
                crate::steps_plain::plain_step_to_yaml(install_rust)?,
                crate::steps_plain::plain_step_to_yaml(probe)?,
            ]),
        ),
    ]);
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(FRESHNESS_WORKFLOW_NAME)),
        (
            "on".to_owned(),
            Yaml::Map(vec![
                ("schedule".to_owned(), Yaml::Seq(crons)),
                ("workflow_dispatch".to_owned(), Yaml::Map(Vec::new())),
            ]),
        ),
        ("permissions".to_owned(), read_only_permissions()),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                ("group".to_owned(), Yaml::str(FRESHNESS_CONCURRENCY_GROUP)),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![(FRESHNESS_JOB_ID.to_owned(), job)]),
        ),
    ]))
}

/// Read-only top-level/job permissions: the probe only reads.
fn read_only_permissions() -> Yaml {
    Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))])
}

#[cfg(test)]
#[path = "freshness_tests.rs"]
mod tests;
