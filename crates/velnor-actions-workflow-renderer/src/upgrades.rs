// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::config::{UPGRADES_WORKFLOW_PATH, UpgradesConfig};

use crate::{MiseSetup, RenderError, guard, marker, render::RenderedFile, steps, yaml::Yaml};

pub const UPGRADES_WORKFLOW_NAME: &str = "Velnor Upgrades";
pub const UPGRADES_BRANCH: &str = "velnor/upgrades";
pub const UPGRADES_CONCURRENCY_GROUP: &str = "velnor-upgrades";
pub const UPGRADES_ARTIFACT: &str = "velnor-upgrade";
pub const CARGO_EDIT_VERSION: &str = "0.13.13";

#[path = "upgrades_jobs.rs"]
mod jobs;
#[path = "upgrades_scripts.rs"]
mod scripts;
use jobs::{env, gate_job, prepare_job, publish_job, verify_job};

const CATALOG_BUMPER: &str = include_str!("../../../scripts/upgrades/bump-gradle-catalog.py");
const WORK_DIR: &str = "${{ runner.temp }}/velnor-upgrades";
const ON_DEFAULT_BRANCH: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";

#[derive(Debug, Clone)]
pub struct UpgradesSpec {
    pub config: UpgradesConfig,
    pub runs_on: String,
    pub checkout_uses: String,
    pub mise_setup: MiseSetup,
    pub gh_version: String,
    pub generator_version: String,
    pub generate_tools: Vec<String>,
    pub verify_rust_script: String,
    pub jactionlint_verify: String,
}

impl UpgradesSpec {
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
        if !self.verify_rust_script.contains("${VELNOR_TOOL_RUST}") {
            return Err(RenderError::BadCommand("bad_verify_rust_script".to_owned()));
        }
        if self.jactionlint_verify.trim().is_empty()
            || self.jactionlint_verify.contains("${{")
            || crate::selected_mise_verifier::validate_argv(&[
                "sh".to_owned(),
                "-c".to_owned(),
                self.jactionlint_verify.clone(),
            ])
            .is_err()
        {
            return Err(RenderError::BadCommand("bad_jactionlint_verify".to_owned()));
        }
        Ok(())
    }
}

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
