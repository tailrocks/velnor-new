// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{GeneratorLock, Job, VelnorConfig, WorkflowPolicy};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::catalog::lock::ApprovedGeneratorRelease;

mod check_mise;
mod graph;
mod inputs;
mod plan;
pub mod support_commands;
mod task_policies;
mod task_sources;
mod tools;

pub(crate) use graph::AuthorityBinding;
pub use inputs::unique_dependency_seed_job_ids;
pub use inputs::{
    CargoWorkspaceFact, GeneratedSourceFact, GeneratedWorkflowKind, GradleRootFact, LogicalJobFact,
    LogicalJobSource, NativeTaskFact, RustTaskFact, SuitePhase, TofuTaskFact, WorkspaceFormatFact,
};
pub use task_sources::WorkflowSourceFact;
pub use tools::catalog_tool_pins;
pub use tools::{NativeRustProvider, NativeRustSource, NativeToolOrigin, NativeToolPin};

#[derive(Debug)]
pub enum NativeAuthorityError {
    Contract(velnor_actions_contract::ContractError),
    Render(crate::RenderError),
    Release(crate::release_helpers::ReleaseHelperError),
    Job(crate::native_job_builders::NativeJobBuilderError),
    Tool(velnor_actions_mise::MiseError),
    Support(support_commands::SupportCommandError),
}

impl From<crate::RenderError> for NativeAuthorityError {
    fn from(error: crate::RenderError) -> Self {
        Self::Render(error)
    }
}
impl From<velnor_actions_contract::ContractError> for NativeAuthorityError {
    fn from(error: velnor_actions_contract::ContractError) -> Self {
        Self::Contract(error)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NativeAuthorityInput<'a> {
    pub config: &'a VelnorConfig,
    pub runner: &'a str,
    pub published_generator_lock: Option<&'a GeneratorLock>,
    pub approved_release: Option<ApprovedGeneratorRelease<'a>>,
    pub consumer_manifest_json: Option<&'a str>,
    pub tool_pins: &'a [NativeToolPin],
    pub rust_source: NativeRustSource,
    pub rust_provider: NativeRustProvider,
    pub cargo_workspaces: &'a [CargoWorkspaceFact],
    pub workflow_sources: &'a [WorkflowSourceFact],
    pub generated_sources: &'a [GeneratedSourceFact],
    pub rust_tasks: &'a [RustTaskFact],
    pub tofu_tasks: &'a [TofuTaskFact],
    pub gradle_roots: &'a [GradleRootFact],
    pub logical_jobs: &'a [LogicalJobFact],
    pub workspace_format: Option<&'a WorkspaceFormatFact>,
    pub tools: crate::native_job_builders::PlanJobToolNeeds,
    pub rust_detected: bool,
}

#[derive(Debug, Clone)]
pub struct NativeAuthority {
    authority: crate::render::CanonicalAuthoritySteps,
    runtime: crate::render::CanonicalRuntimeInputs,
    required: Job,
    producer_plan: Option<Job>,
    mise: crate::MiseSetup,
}

impl NativeAuthority {
    pub fn build(input: NativeAuthorityInput<'_>) -> Result<Self, NativeAuthorityError> {
        input.config.validate("config.toml")?;
        crate::guard::validate_runs_on(input.runner)?;
        let target = velnor_actions_contract::ReleaseTarget::for_runner_label(input.runner)
            .ok_or_else(|| invalid(format!("unsupported_target_for_runner:{}", input.runner)))?;
        let catalog = tools::catalog(&input)?;
        let fetch_roots = inputs::fetch_roots(input.cargo_workspaces)?;
        let (required_ids, tofu_roots) = inputs::validate_graph(&input)?;
        let producer = input.config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1;
        let (version, acquire) = if producer {
            Self::producer_release_acquire(&input, target)?
        } else {
            let version = env!("CARGO_PKG_VERSION").to_owned();
            let acquire = crate::release_helpers::consumer_acquire_for_target(
                target,
                &version,
                input.consumer_manifest_json,
            )
            .map_err(NativeAuthorityError::Release)?;
            (version, acquire)
        };
        let helper_catalog = plan::helper_catalog(&input, &catalog)?;
        let mise = tools::mise_setup(input.config, target)?;
        let mut runtime = runtime(&catalog, &version, input.tools.rust.has_compiler())?;
        let required_acquire =
            required_acquisition(producer, target, acquire.clone(), &mut runtime)?;
        let mut required = plan::required_job(&input, &required_ids, required_acquire)?;
        let mut producer_plan = if producer {
            Some(plan::build(
                &input,
                &helper_catalog,
                acquire,
                &fetch_roots,
                &tofu_roots,
                &version,
            )?)
        } else {
            None
        };
        if input.config.workflow.checkout_lfs {
            plan::checkout_lfs(&mut required);
            if let Some(plan) = &mut producer_plan {
                plan::checkout_lfs(plan);
            }
        }
        let mut jobs = BTreeMap::from([(crate::render::FINAL_JOB_ID.to_owned(), required.clone())]);
        if let Some(plan) = &producer_plan {
            jobs.insert(crate::render::PLAN_JOB_ID.to_owned(), plan.clone());
        }
        let (binding, variants) = graph::build(&input, &jobs, &catalog)?;
        let setup = crate::setup::mise_setup_step(&mise)?;
        let authority = crate::render::CanonicalAuthoritySteps::from_native_jobs_inner(
            &jobs,
            input.config.workflow.policy,
            Some(setup),
            &variants,
            runtime.clone(),
            binding,
        )?;
        Ok(Self {
            authority,
            runtime,
            required,
            producer_plan,
            mise,
        })
    }

    fn producer_release_acquire(
        input: &NativeAuthorityInput<'_>,
        target: velnor_actions_contract::ReleaseTarget,
    ) -> Result<(String, velnor_actions_contract::Step), NativeAuthorityError> {
        let lock = input
            .published_generator_lock
            .ok_or_else(|| invalid("required_verdict_requires_published_generator_lock"))?;
        match input.approved_release {
            Some(approved) => {
                crate::release_helpers::validate_approved_generator_lock(lock, &approved)
                    .map_err(NativeAuthorityError::Release)?;
            }
            None => {
                return Err(invalid("controller_approvals:missing_approval"));
            }
        }
        let staged = format!(
            "{}{}",
            crate::steps::STAGED_BINARY_PREFIX,
            lock.generator.version
        );
        let acquire = crate::release_helpers::lock_acquire_for_target(lock, target, &staged)
            .map_err(NativeAuthorityError::Release)?;
        Ok((lock.generator.version.clone(), acquire))
    }

    #[must_use]
    pub fn authority(&self) -> crate::render::CanonicalAuthoritySteps {
        self.authority.clone()
    }
    #[must_use]
    pub fn runtime(&self) -> crate::render::CanonicalRuntimeInputs {
        self.runtime.clone()
    }
    #[must_use]
    pub fn required_job(&self) -> Job {
        self.required.clone()
    }
    #[must_use]
    pub fn producer_plan_job(&self) -> Option<Job> {
        self.producer_plan.clone()
    }
    #[must_use]
    pub fn mise_setup(&self) -> crate::MiseSetup {
        self.mise.clone()
    }
}

fn required_acquisition(
    producer: bool,
    target: velnor_actions_contract::ReleaseTarget,
    acquire: velnor_actions_contract::Step,
    runtime: &mut crate::render::CanonicalRuntimeInputs,
) -> Result<velnor_actions_contract::Step, NativeAuthorityError> {
    if !producer {
        return Ok(acquire);
    }
    let manifest = velnor_actions_mise::catalog::lock::published_verdict_manifest();
    *runtime = runtime
        .clone()
        .with_required_helper_version(&manifest.version)?;
    crate::release_helpers::published_verdict_acquire_for_target(target)
        .map_err(NativeAuthorityError::Release)
}

fn runtime(
    catalog: &ToolCatalog,
    version: &str,
    rust: bool,
) -> Result<crate::render::CanonicalRuntimeInputs, NativeAuthorityError> {
    let mut env = velnor_actions_mise::ISOLATION_ENV
        .into_iter()
        .chain(velnor_actions_mise::NO_AUTO_INSTALL_ENV)
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect::<BTreeMap<_, _>>();
    if rust {
        let homes = velnor_actions_mise::ToolHomes::runner_temp();
        env = crate::toolchain_env::checked_task_env(
            &env,
            homes.rustup_home(),
            homes.cargo_home(),
            catalog.rustup_toolchain_env().unwrap_or(""),
        )?;
    }
    crate::render::CanonicalRuntimeInputs::new(
        env,
        version.to_owned(),
        format!("{}{version}", crate::steps::STAGED_BINARY_PREFIX),
        "${{ runner.temp }}/velnor/request".to_owned(),
        crate::steps::CHECKOUT_USES.to_owned(),
        false,
    )
    .map_err(Into::into)
}

fn invalid(problem: impl Into<String>) -> NativeAuthorityError {
    NativeAuthorityError::Render(crate::RenderError::InvalidWorkflow(problem.into()))
}
