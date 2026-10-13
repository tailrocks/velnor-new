//! Typed `Verify toolchain` step requests (task §2 step 2).
//!
//! Builds identity probes for Rust, the selected compile driver, the selected test
//! runner, the target, and the runner platform, and reports optional
//! advisory tool-file findings. The requested compile route is selected through
//! [`select_route`]; probes are identity
//! invocations (`--version`), never builds. Findings arrive validated
//! from the owning adapters; this step only reports them.

use std::ffi::OsString;

use velnor_actions_contract::Finding;

use crate::catalog::ToolCatalog;
use crate::command::{IsolatedCommand, NO_AUTO_INSTALL_ENV};
use crate::error::MiseError;
use crate::nextest::NextestDriver;
use crate::preflight::{RouteDriver, RouteSelection, select_route};
use crate::requests::PinnedToolExec;
use crate::steps::{ToolHomes, validate_step_token};

/// Contract-fixed display name of the task-execution §2 step 2.
/// Emitters use this const, never a retyped string.
pub const VERIFY_TOOLCHAIN_STEP: &str = "Verify toolchain";

/// Selected test runner (`RustExecutionProfile` `test_runner`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestRunner {
    /// Cargo's own test runner: `cargo test`.
    CargoTest,
    /// Nextest runner: `cargo nextest` or `mbx nextest` by driver.
    CargoNextest,
}

impl TestRunner {
    /// Execution-profile spelling of the selected runner.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CargoTest => "cargo_test",
            Self::CargoNextest => "cargo_nextest",
        }
    }
}

/// Validated inputs for one `Verify toolchain` step.
#[derive(Debug, Clone)]
pub struct VerifySpec<'a> {
    /// Compile route selection: MBX requires an action-owned binary on PATH.
    pub driver: RouteDriver,
    /// Selected test runner.
    pub runner: TestRunner,
    /// Adapter-reported compiler/cache format; empty fails, never guesses.
    pub format: &'a str,
    /// Adapter-reported generation; empty fails, never guesses.
    pub generation: &'a str,
    /// Explicit `--target` triple, or `host` when the entry omits it.
    pub target: &'a str,
    /// Runner platform label the step executes on.
    pub platform: &'a str,
    /// Velnor-owned tool homes for the step env.
    pub homes: ToolHomes,
    /// Optional advisory tool-file findings reported with the step.
    pub findings: Vec<Finding>,
}

/// Toolchain probes plus their recorded target, platform, and findings.
///
/// Probe verification is one argv per probe: the route probe always,
/// plus the runner probe when Nextest is selected (`cargo_test` is
/// covered by the route probe, which already invokes its runner).
/// Target, platform, and findings are recorded data the emitter
/// attaches to the matrix entry and report; they alter no argv.
#[derive(Debug, Clone)]
pub struct VerifyToolchain {
    /// Selected compile route: identity inputs plus the exact requested probe.
    route: RouteSelection,
    /// Selected test runner.
    runner: TestRunner,
    /// Runner identity probe; `None` when the route probe covers it.
    runner_probe: Option<PinnedToolExec>,
    /// Recorded target triple or `host`.
    target: String,
    /// Recorded runner platform label.
    platform: String,
    /// Owned homes carried by the step env.
    homes: ToolHomes,
    /// Advisory tool-file findings reported with the step.
    findings: Vec<Finding>,
}

impl VerifyToolchain {
    /// Select the route and record target, platform, homes, and findings.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidStepInput`] for a blank or hostile
    /// target/platform, [`MiseError::Contract`] for an unreportable
    /// format/generation or an invalid finding, and
    /// [`MiseError::ForbiddenPayload`] only if a fixed probe were
    /// forbidden, which construction rules out.
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

    /// Contract-fixed display name of the emitted step.
    #[must_use]
    pub fn step_name() -> &'static str {
        VERIFY_TOOLCHAIN_STEP
    }

    /// Selected compile route. An external action prerequisite is not executed here.
    #[must_use]
    pub fn route(&self) -> &RouteSelection {
        &self.route
    }

    /// Selected route driver; external action prerequisites are not executed here.
    #[must_use]
    pub fn driver(&self) -> RouteDriver {
        self.route.driver()
    }

    /// Selected exact tool identities, including action-owned prerequisites.
    #[must_use]
    pub fn identity_specs(&self) -> &[String] {
        self.route.identity_specs()
    }

    /// Exact tool selectors passed to Mise for the route probe.
    #[must_use]
    pub fn probe_specs(&self) -> &[String] {
        self.route.probe_specs()
    }

    /// Cache-format identity over the reported format and generation.
    #[must_use]
    pub fn cache_format_id(&self) -> &str {
        self.route.cache_format_id()
    }

    /// Selected test runner.
    #[must_use]
    pub fn runner(&self) -> TestRunner {
        self.runner
    }

    /// Recorded target triple or `host`.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Recorded runner platform label.
    #[must_use]
    pub fn platform(&self) -> &str {
        &self.platform
    }

    /// Owned homes carried by the step env.
    #[must_use]
    pub fn homes(&self) -> &ToolHomes {
        &self.homes
    }

    /// Advisory tool-file findings reported with the step.
    #[must_use]
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// Probe argv vectors including the program: route probe first,
    /// runner probe second when Nextest is selected.
    #[must_use]
    pub fn probes(&self, catalog: &ToolCatalog) -> Vec<Vec<OsString>> {
        let mut probes = vec![self.route.invocation(catalog)];
        if let Some(probe) = &self.runner_probe {
            probes.push(probe.argv(catalog));
        }
        probes
    }

    /// Full step env: isolation plus install-disable plus owned homes.
    ///
    /// Verification runs with implicit installation disabled, so a
    /// missing tool fails as a preparation error. Matches every
    /// [`Self::commands`] spawner env; pinned by test.
    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        let mut env = IsolatedCommand::env_overlay();
        for (key, value) in NO_AUTO_INSTALL_ENV {
            env.push((OsString::from(key), OsString::from(value)));
        }
        env.extend(self.homes.env(catalog));
        env
    }

    /// Isolated commands running each probe, one per [`Self::probes`].
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if a fixed probe were
    /// empty, which construction rules out.
    pub fn commands(&self, catalog: &ToolCatalog) -> Result<Vec<IsolatedCommand>, MiseError> {
        let homes = self.homes.env(catalog);
        let mut commands = vec![self.route.command(catalog)?.with_env(&homes)?];
        if let Some(probe) = &self.runner_probe {
            commands.push(probe.command(catalog)?.with_env(&homes)?);
        }
        Ok(commands)
    }
}

/// Identity probe for the selected runner.
///
/// `cargo_test` needs none: the route probe already invokes its
/// runner (`cargo` or `mbx`). Nextest gets `<driver> nextest
/// --version` under the exact driver-plus-runner selectors.
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
    prefix
        .exec(vec![OsString::from("nextest"), OsString::from("--version")])
        .map(Some)
}
