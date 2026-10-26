// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use velnor_actions_contract::JobTimeout;

use velnor_actions_contract::{FRESHNESS_WORKFLOW_PATH, ScheduleTrigger, Step, StepRole};

use crate::{
    MiseSetup, RenderError, guard, marker, mise_setup_step,
    render::RenderedFile,
    steps::{self, action_step, shell_step},
    yaml::Yaml,
};

pub const FRESHNESS_WORKFLOW_NAME: &str = "Upstream Freshness";
pub const FRESHNESS_JOB_ID: &str = "freshness";
pub const FRESHNESS_JOB_NAME: &str = "Freshness";
pub const FRESHNESS_SCRIPT: &str = "scripts/check-freshness.sh";
pub const FRESHNESS_PROBE_FLAG: &str = "--check-upstream";
pub const FRESHNESS_CONCURRENCY_GROUP: &str = "freshness";
pub const FRESHNESS_TIMEOUT_MINUTES: u16 = JobTimeout::DEFAULT.minutes();

#[derive(Debug, Clone)]
pub struct FreshnessSpec {
    pub schedule: ScheduleTrigger,
    pub runs_on: String,
    pub checkout_uses: String,
    pub mise_setup: MiseSetup,
    pub rust_version: String,
    pub verify_rust_argv: Vec<String>,
    pub generator_version: String,
}

impl FreshnessSpec {
    pub fn validate(&self) -> Result<(), RenderError> {
        self.schedule.validate().map_err(RenderError::Contract)?;
        marker::validate_version(&self.generator_version)?;
        guard::validate_runs_on(&self.runs_on)?;
        steps::validate_uses(&self.checkout_uses)?;
        self.mise_setup.validate()?;
        if !crate::commands::is_exact_or_project_version(&self.rust_version) {
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
    let verify_rust = crate::verifier_composite::verifier_shell_step(
        velnor_actions_contract::StepRole::VerifyRustManifest,
        crate::VERIFY_RUST_TOOLCHAIN_STEP,
        spec.verify_rust_argv.clone(),
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
    let document = freshness_document(
        spec,
        &checkout,
        &setup_mise,
        &[install_rust, verify_rust],
        &probe,
    )?;
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

fn freshness_document(
    spec: &FreshnessSpec,
    checkout: &Step,
    setup_mise: &Step,
    install_rust: &[Step],
    probe: &Step,
) -> Result<Yaml, RenderError> {
    let mut sequence = vec![checkout.clone(), setup_mise.clone()];
    sequence.extend(install_rust.iter().cloned());
    sequence.push(probe.clone());
    velnor_actions_contract::workflow::step_identity::validate_step_sequence(
        &sequence,
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
            Yaml::Int(i64::from(FRESHNESS_TIMEOUT_MINUTES)),
        ),
        ("permissions".to_owned(), read_only_permissions()),
        (
            "steps".to_owned(),
            Yaml::Seq(
                sequence
                    .iter()
                    .map(crate::steps_plain::plain_step_to_yaml)
                    .collect::<Result<_, _>>()?,
            ),
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

fn read_only_permissions() -> Yaml {
    Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))])
}
