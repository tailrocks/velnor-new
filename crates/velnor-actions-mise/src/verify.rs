// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};

use velnor_actions_contract::Finding;

use crate::catalog::ToolCatalog;
use crate::command::{IsolatedCommand, NO_AUTO_INSTALL_ENV};
use crate::error::MiseError;
use crate::nextest::NextestDriver;
use crate::preflight::{RouteDriver, RouteSelection, select_route};
use crate::requests::PinnedToolExec;
use crate::steps::{ToolHomes, validate_step_token};

pub const VERIFY_TOOLCHAIN_STEP: &str = "Verify toolchain";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestRunner {
    CargoTest,
    CargoNextest,
}

impl TestRunner {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CargoTest => "cargo_test",
            Self::CargoNextest => "cargo_nextest",
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerifySpec<'a> {
    pub driver: RouteDriver,
    pub runner: TestRunner,
    pub format: &'a str,
    pub generation: &'a str,
    pub target: &'a str,
    pub platform: &'a str,
    pub homes: ToolHomes,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone)]
pub struct VerifyToolchain {
    route: RouteSelection,
    runner: TestRunner,
    runner_probe: Option<PinnedToolExec>,
    target: String,
    platform: String,
    homes: ToolHomes,
    findings: Vec<Finding>,
}

impl VerifyToolchain {
    pub fn new(catalog: &ToolCatalog, spec: VerifySpec<'_>) -> Result<Self, MiseError> {
        validate_step_token("target", spec.target)?;
        validate_step_token("platform", spec.platform)?;
        for finding in &spec.findings {
            finding.validate().map_err(|err| MiseError::Contract {
                problem: err.to_string(),
            })?;
        }
        let route = select_route(catalog, spec.driver, spec.format, spec.generation)?;
        let runner_probe = runner_probe(spec.driver, spec.runner)?;
        Ok(Self {
            route,
            runner: spec.runner,
            runner_probe,
            target: spec.target.to_owned(),
            platform: spec.platform.to_owned(),
            homes: spec.homes,
            findings: spec.findings,
        })
    }

    #[must_use]
    pub fn step_name() -> &'static str {
        VERIFY_TOOLCHAIN_STEP
    }

    #[must_use]
    pub fn route(&self) -> &RouteSelection {
        &self.route
    }

    #[must_use]
    pub fn driver(&self) -> RouteDriver {
        self.route.driver()
    }

    #[must_use]
    pub fn identity_specs(&self) -> &[String] {
        self.route.identity_specs()
    }

    #[must_use]
    pub fn probe_specs(&self) -> &[String] {
        self.route.probe_specs()
    }

    #[must_use]
    pub fn cache_format_id(&self) -> &str {
        self.route.cache_format_id()
    }

    #[must_use]
    pub fn runner(&self) -> TestRunner {
        self.runner
    }

    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    #[must_use]
    pub fn platform(&self) -> &str {
        &self.platform
    }

    #[must_use]
    pub fn homes(&self) -> &ToolHomes {
        &self.homes
    }

    #[must_use]
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    #[must_use]
    pub fn probes(&self, catalog: &ToolCatalog) -> Vec<Vec<OsString>> {
        let mut probes = vec![self.route.invocation(catalog)];
        if let Some(probe) = &self.runner_probe {
            probes.push(probe.argv(catalog));
        }
        probes
    }

    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        let mut env = IsolatedCommand::env_overlay();
        for (key, value) in NO_AUTO_INSTALL_ENV {
            env.push((OsString::from(key), OsString::from(value)));
        }
        env.extend(self.homes.env(catalog));
        env
    }

    pub fn commands(&self, catalog: &ToolCatalog) -> Result<Vec<IsolatedCommand>, MiseError> {
        let homes = self.homes.env(catalog);
        let mut commands = vec![self.route.command(catalog)?.with_env(&homes)?];
        if let Some(probe) = &self.runner_probe {
            commands.push(probe.command(catalog)?.with_env(&homes)?);
        }
        Ok(commands)
    }
}

fn runner_probe(
    driver: RouteDriver,
    runner: TestRunner,
) -> Result<Option<PinnedToolExec>, MiseError> {
    if runner == TestRunner::CargoTest {
        return Ok(None);
    }
    let prefix = match driver {
        RouteDriver::Cargo => NextestDriver::Cargo,
        RouteDriver::Mbx => NextestDriver::Mbx,
    };
    PinnedToolExec::new(
        prefix.tools(),
        OsStr::new(prefix.program()),
        vec![OsString::from("nextest"), OsString::from("--version")],
    )
    .map(Some)
}
