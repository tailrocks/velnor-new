// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{MiseTaskBootstrapPolicy, VerificationTaskPolicy, mise_task_bootstrap};
use crate::{RenderError, shell_step};
use std::collections::BTreeMap;
use velnor_actions_contract::{Job, JobTimeout, PermissionLevel, Permissions, Step};
use velnor_actions_mise::PinnedTool;

pub(crate) const RUNNER_FAMILY_POLICY_ID: &str = "runner-image-family";
const SCRIPT: &str = "scripts/check-runner-image-candidate.sh";
pub const FAMILY_PREFETCH_SOURCES: [&str; 3] = [
    "scripts/generator-release/cargo-git-handoff.py",
    "scripts/generator-release/cargo-source-handoff.sh",
    "scripts/generator-release/source-qualification.sh",
];

#[derive(Debug, Clone)]
pub struct RunnerFamilyPolicy {
    pub bootstrap: VerificationTaskPolicy,
    pub script_sha256: String,
    pub prefetch_sha256: BTreeMap<String, String>,
}

pub fn build_runner_family_job(
    policy: &RunnerFamilyPolicy,
    checkout: &str,
) -> Result<Job, RenderError> {
    if policy.bootstrap.task.id != RUNNER_FAMILY_POLICY_ID {
        return Err(RenderError::InvalidWorkflow(
            "runner_family_policy_id".into(),
        ));
    }
    if policy.script_sha256.len() != 64
        || !policy
            .script_sha256
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(RenderError::InvalidWorkflow(
            "runner_family_script_digest".into(),
        ));
    }
    if !policy
        .bootstrap
        .selected_tools
        .iter()
        .any(|tool| tool.key == "rust")
        || !policy.bootstrap.selected_tools.iter().any(|tool| {
            tool.key == "mr-boxington" && tool.backend == "packslip:github.com/jdx/mr-boxington"
        })
    {
        return Err(RenderError::InvalidWorkflow(
            "runner_family_tools_missing".into(),
        ));
    }
    let id = velnor_actions_contract::config::RUNNER_IMAGE_FAMILY_JOB_ID;
    let guard = source_guard(policy)?;
    let bootstrap = prepare_bootstrap(policy)?;
    let catalog = family_catalog(&bootstrap)?;
    let verifier_catalog = catalog
        .clone()
        .with_project_tool(PinnedTool::Rust, catalog.version(PinnedTool::Rust))
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?
        .for_emission();
    let mut task = mise_task_bootstrap::steps(
        &bootstrap,
        &policy.bootstrap.mise_setup,
        checkout,
        false,
        true,
    )?;
    task.insert(
        1,
        shell_step(
            "Verify runner family task source",
            vec!["bash".into(), "-c".into(), guard],
            BTreeMap::new(),
        )?,
    );
    add_family_sources_and_verification(&mut task, &bootstrap, &verifier_catalog)?;
    task.push(upload_step()?);
    Ok(Job {
        services: Vec::new(),
        outputs: BTreeMap::new(),
        display_name: "Qualify runner image family".into(),
        runs_on: "ubuntu-26.04".into(),
        timeout_minutes: JobTimeout::new(120).map_err(RenderError::Contract)?,
        needs: vec![crate::render::PLAN_JOB_ID.to_owned()],
        condition: Some(crate::plan_gate::not_affected_gate(id)),
        permissions: Some(Permissions {
            contents: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            attest: PermissionLevel::None,
            actions: PermissionLevel::None,
            packages: PermissionLevel::None,
        }),
        cache_access: velnor_actions_contract::CacheAccess::Read,
        environment: None,
        env: velnor_actions_contract::JobEnv::RustToolchain,
        check_runner: None,
        steps: task,
    })
}

fn source_guard(policy: &RunnerFamilyPolicy) -> Result<String, RenderError> {
    let mut commands = vec![format!(
        "set -eu; printf '%s  %s\\n' '{}' '{}' | sha256sum --check --status",
        policy.script_sha256, SCRIPT
    )];
    if policy.prefetch_sha256.len() != FAMILY_PREFETCH_SOURCES.len() {
        return Err(RenderError::InvalidWorkflow(
            "runner_family_prefetch_sources".into(),
        ));
    }
    for path in FAMILY_PREFETCH_SOURCES {
        let digest = policy
            .prefetch_sha256
            .get(path)
            .ok_or_else(|| RenderError::InvalidWorkflow("runner_family_prefetch_sources".into()))?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(RenderError::InvalidWorkflow(
                "runner_family_script_digest".into(),
            ));
        }
        commands.push(format!(
            "printf '%s  %s\\n' '{digest}' '{path}' | sha256sum --check --status"
        ));
    }
    Ok(commands.join("; "))
}

fn add_family_sources_and_verification(
    task: &mut Vec<Step>,
    bootstrap: &MiseTaskBootstrapPolicy,
    verifier_catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<(), RenderError> {
    let mut sources =
        crate::plan_sources::source_cache::restore_step_for_crate(&[".".to_owned()], true)
            .map_err(|error| match error {
                crate::plan_sources::PlanSourcesError::Contract(error) => {
                    RenderError::Contract(error)
                }
                crate::plan_sources::PlanSourcesError::ShellRender(error)
                | crate::plan_sources::PlanSourcesError::Render(error) => error,
                crate::plan_sources::PlanSourcesError::CacheSourcesPolicy(error)
                | crate::plan_sources::PlanSourcesError::CacheActionRef(error) => {
                    RenderError::InvalidWorkflow(error)
                }
            })?
            .ok_or_else(|| RenderError::InvalidWorkflow("runner_family_sources_missing".into()))?;
    sources.id = Some(velnor_actions_contract::StepId::SourcesRestore);
    task.insert(2, sources);
    let run_index = task
        .iter()
        .position(|step| step.name == bootstrap.run_step_name)
        .ok_or_else(|| RenderError::InvalidWorkflow("runner_family_task_missing".into()))?;
    let mut base = BTreeMap::new();
    for (key, value) in velnor_actions_mise::ISOLATION_ENV
        .iter()
        .chain(velnor_actions_mise::NO_AUTO_INSTALL_ENV.iter())
    {
        base.insert((*key).to_owned(), (*value).to_owned());
    }
    let homes = velnor_actions_mise::ToolHomes::runner_temp();
    let mut verify_env = crate::toolchain_env::checked_task_env(
        &base,
        homes.rustup_home(),
        homes.cargo_home(),
        verifier_catalog.rustup_toolchain_env().unwrap_or(""),
    )?;
    for (native, mise) in [
        ("CARGO_HOME", "MISE_CARGO_HOME"),
        ("RUSTUP_HOME", "MISE_RUSTUP_HOME"),
    ] {
        let value = verify_env
            .get(mise)
            .ok_or_else(|| {
                RenderError::InvalidWorkflow("runner_family_verifier_home_missing".into())
            })?
            .clone();
        verify_env.insert(native.to_owned(), value);
    }
    let verifier = crate::verifier_composite::verifier_shell_step(
        velnor_actions_contract::StepRole::VerifyRustManifest,
        crate::VERIFY_RUST_TOOLCHAIN_STEP,
        crate::native_verifiers::verify_rust_argv(verifier_catalog),
        verify_env.clone(),
    )?;
    task.insert(run_index, verifier.clone());
    task.insert(
        run_index + 1,
        mise_task_bootstrap::prepare_target_step(bootstrap)?,
    );
    task.insert(run_index + 2, verifier);
    verify_env.insert("VELNOR_PREFETCH_RUST_ONLY".into(), "1".into());
    verify_env.insert(
        "VELNOR_RUST_VERSION".into(),
        verifier_catalog.version(PinnedTool::Rust).into(),
    );
    task.insert(
        run_index + 3,
        crate::runner_family_sources::step(verify_env)?,
    );
    Ok(())
}

fn prepare_bootstrap(policy: &RunnerFamilyPolicy) -> Result<MiseTaskBootstrapPolicy, RenderError> {
    let mut bootstrap = MiseTaskBootstrapPolicy::from_verification(&policy.bootstrap);
    bootstrap.shared_cargo_sources = true;
    bootstrap.runner_family_script = true;
    bootstrap.run_step_name = "Run runner image candidate";
    let catalog = family_catalog(&bootstrap)?;
    let target = velnor_actions_mise::PrepareRustTarget::new(
        "x86_64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
    )
    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    bootstrap.prepare_rust_target = Some(
        target
            .argv(&catalog)
            .into_iter()
            .map(|argument| {
                argument
                    .into_string()
                    .map_err(|_| RenderError::InvalidWorkflow("runner_family_target_argv".into()))
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    mise_task_bootstrap::validate_policy(&bootstrap)?;
    Ok(bootstrap)
}

fn family_catalog(
    bootstrap: &MiseTaskBootstrapPolicy,
) -> Result<velnor_actions_mise::ToolCatalog, RenderError> {
    let rust = bootstrap
        .selected_tools
        .iter()
        .find(|tool| tool.key == "rust")
        .ok_or_else(|| RenderError::InvalidWorkflow("runner_family_tools_missing".into()))?;
    let mbx = bootstrap
        .selected_tools
        .iter()
        .find(|tool| tool.key == "mr-boxington")
        .ok_or_else(|| RenderError::InvalidWorkflow("runner_family_tools_missing".into()))?;
    let pinned = velnor_actions_mise::ToolCatalog::pinned();
    velnor_actions_mise::ToolCatalog::new(
        &rust.version,
        &mbx.version,
        pinned.version(PinnedTool::Gh),
        pinned.version(PinnedTool::Jactionlint),
        pinned.version(PinnedTool::Shellcheck),
        pinned.version(PinnedTool::Nextest),
        pinned.version(PinnedTool::Opentofu),
    )
    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))
}

fn upload_step() -> Result<Step, RenderError> {
    let mut upload = crate::steps::action_step(
        "Upload runner image family",
        crate::steps::UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "name".into(),
                "image-assets-${{ github.run_attempt }}".into(),
            ),
            (
                "path".into(),
                velnor_actions_contract::RUNNER_IMAGE_FAMILY_ARTIFACT_NAMES
                    .iter()
                    .map(|name| {
                        format!("${{{{ runner.temp }}}}/runner-image-assets-parent/assets/{name}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            ("if-no-files-found".into(), "error".into()),
            ("compression-level".into(), "0".into()),
        ]),
    )?;
    upload.condition =
        Some("github.event_name == 'push' && github.ref == 'refs/heads/main'".into());
    Ok(upload)
}

pub(crate) fn validate_runner_family_job(
    jobs: &BTreeMap<String, Job>,
    policy: &RunnerFamilyPolicy,
    checkout: &str,
) -> Result<Vec<String>, RenderError> {
    let id = velnor_actions_contract::config::RUNNER_IMAGE_FAMILY_JOB_ID;
    let actual = jobs
        .get(id)
        .ok_or_else(|| RenderError::InvalidWorkflow("runner_family_job_missing".into()))?;
    let expected = build_runner_family_job(policy, checkout)?;
    if actual.display_name != expected.display_name
        || actual.runs_on != expected.runs_on
        || actual.timeout_minutes != expected.timeout_minutes
        || actual.needs != expected.needs
        || actual.condition != expected.condition
        || actual.permissions != expected.permissions
        || actual.environment != expected.environment
        || actual.steps != expected.steps
    {
        return Err(RenderError::InvalidWorkflow(
            "runner_family_job_contract".into(),
        ));
    }
    Ok(vec![id.into()])
}
