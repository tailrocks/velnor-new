//! Release job assembly: pinned coordinator argv plus the role set.
//!
//! Every release-plz invocation runs through pinned Mise tools
//! (`rust` plus `release-plz`, exact versions from the compiled
//! catalog) with an explicit `--config` binding plus the `GIT_TOKEN`
//! forge binding; no argv is invented here — shapes come from the Mise
//! release-plz builders. Jobs read configs from the policy checkout
//! (event commit) while preflight and publishers run release-plz
//! against the exact-source checkout. OIDC mode assembles four jobs;
//! bootstrap-token mode adds the fifth publish job carrying the single
//! registry-token binding.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use velnor_actions_contract_config::config::{ReleaseAuthentication, RustReleaseConfig};
use velnor_actions_contract_workflow::{JobTimeout, Step};
use velnor_actions_mise::catalog::release_plz::{ReleasePrRequest, ReleaseRequest as PlzRelease};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_workflow_renderer::release_jobs::{ReleaseJobSpec, ReleaseRole};
use velnor_actions_workflow_renderer::release_permissions::JobPermissions;
use velnor_actions_workflow_renderer::release_tree::{
    RELEASE_BOOTSTRAP_CONFIG_PATH, RELEASE_CONFIG_PATH,
};
use velnor_actions_workflow_steps::{MiseSetup, ambient_shell_step, mise_setup_step};

use crate::OrchestratorError;
use crate::release_checkouts::{forge_env, policy_checkout, source_checkout, source_manifest};
use crate::utf8::strings_of;

/// Bootstrap token: env key plus its only secret binding.
const BOOTSTRAP_TOKEN_ENV: &str = "CARGO_REGISTRY_TOKEN";
/// Bootstrap token secret reference (env-only, never argv).
const BOOTSTRAP_SECRET_REF: &str = "${{ secrets.CARGO_REGISTRY_TOKEN }}";

/// Release-plz invocation kind per job.
enum Phase {
    /// `release-pr`: preparation only, never publishes.
    PreparePr,
    /// `release`: the publish jobs.
    Publish,
    /// `release --dry-run`: validation without upload.
    DryRun,
}

/// Shared job inputs.
pub(crate) struct JobInputs<'a> {
    /// Validated release section.
    pub(crate) release: &'a RustReleaseConfig,
    /// Exact publish gate condition.
    pub(crate) gate: String,
    /// Approved source SHA every source checkout pins.
    pub(crate) sha: &'a str,
    /// `--registry` value; `None` omits the flag for the cargo-implicit
    /// default (release-plz 0.3.169 cannot resolve that name from config).
    pub(crate) registry: Option<&'a str>,
    /// Literal runner label.
    pub(crate) label: &'a str,
    /// Pinned Mise setup step inputs.
    pub(crate) mise: &'a MiseSetup,
    /// Pinned tool catalog for coordinator argv.
    pub(crate) catalog: &'a ToolCatalog,
}

/// One job's assembled head (checkouts plus setup) and bindings.
struct JobParts<'a> {
    /// Credential-free policy checkout (config reads only).
    policy: Step,
    /// Credentialed policy checkout (preparation pushes its branch).
    policy_push: Step,
    /// Credential-free exact-source checkout (dry-run validation).
    source: Step,
    /// Credentialed exact-source checkout (publishers push tags).
    source_push: Step,
    /// Pinned Mise setup step.
    setup: Step,
    /// Literal runner label.
    label: &'a str,
    /// Exact publish gate condition.
    gate: &'a str,
    /// Protected publishing environment.
    environment: &'a str,
}

/// Assemble the four (OIDC) or five (bootstrap) release jobs.
///
/// Preparation runs `release-pr` on the policy checkout, preflight
/// dry-runs the publish config against the exact source, each publish
/// job releases its own generated config from the exact source, and
/// reconcile dry-runs the normal config against the policy tree.
///
/// # Errors
///
/// Returns render or contract errors for invalid steps or argv.
pub(crate) fn assemble_jobs(
    inputs: &JobInputs<'_>,
) -> Result<BTreeMap<String, ReleaseJobSpec>, OrchestratorError> {
    let parts = JobParts {
        policy: policy_checkout(false)?,
        policy_push: policy_checkout(true)?,
        source: source_checkout(inputs.sha, false)?,
        source_push: source_checkout(inputs.sha, true)?,
        setup: mise_setup_step(inputs.mise)?,
        label: inputs.label,
        gate: &inputs.gate,
        environment: &inputs.release.environment,
    };
    let bootstrap = inputs.release.authentication == ReleaseAuthentication::BootstrapToken;
    let manifest = inputs.release.manifest_path.as_str();
    let sourced = source_manifest(manifest);
    let mut jobs = BTreeMap::new();
    let (prepare, name) = if inputs.release.release_pr {
        (Phase::PreparePr, "Prepare release PR")
    } else {
        (Phase::DryRun, "Validate release")
    };
    let argv = phase_argv(
        &prepare,
        RELEASE_CONFIG_PATH,
        manifest,
        inputs.registry,
        inputs.catalog,
    )?;
    jobs.insert(
        "release-preparation".to_owned(),
        preparation_job(&parts, argv, name)?,
    );
    let preflight_config = if bootstrap {
        RELEASE_BOOTSTRAP_CONFIG_PATH
    } else {
        RELEASE_CONFIG_PATH
    };
    let argv = phase_argv(
        &Phase::DryRun,
        preflight_config,
        &sourced,
        inputs.registry,
        inputs.catalog,
    )?;
    jobs.insert("release-preflight".to_owned(), preflight_job(&parts, argv)?);
    let argv = phase_argv(
        &Phase::Publish,
        RELEASE_CONFIG_PATH,
        &sourced,
        inputs.registry,
        inputs.catalog,
    )?;
    jobs.insert("release-publish".to_owned(), publish_job(&parts, argv)?);
    if bootstrap {
        let argv = phase_argv(
            &Phase::Publish,
            RELEASE_BOOTSTRAP_CONFIG_PATH,
            &sourced,
            inputs.registry,
            inputs.catalog,
        )?;
        jobs.insert(
            "release-publish-bootstrap".to_owned(),
            bootstrap_job(&parts, argv)?,
        );
    }
    let argv = phase_argv(
        &Phase::DryRun,
        RELEASE_CONFIG_PATH,
        manifest,
        inputs.registry,
        inputs.catalog,
    )?;
    jobs.insert(
        "release-reconcile".to_owned(),
        reconcile_job(&parts, argv, bootstrap)?,
    );
    Ok(jobs)
}

/// Fixed coordinator argv through pinned Mise tools.
///
/// # Errors
///
/// Returns contract errors when argv construction or UTF-8 fails.
fn phase_argv(
    phase: &Phase,
    config: &str,
    manifest: &str,
    registry: Option<&str>,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let path = PathBuf::from(config);
    let payload = match phase {
        Phase::PreparePr => ReleasePrRequest::new(path)
            .and_then(|request| request.with_manifest(PathBuf::from(manifest)))
            .and_then(|request| pr_registry(request, registry))
            .map(|request| request.release_pr_argv()),
        Phase::Publish => PlzRelease::release(path)
            .and_then(|request| request.with_manifest(PathBuf::from(manifest)))
            .and_then(|request| plz_registry(request, registry))
            .map(|request| request.release_argv()),
        Phase::DryRun => PlzRelease::dry_run(path)
            .and_then(|request| request.with_manifest(PathBuf::from(manifest)))
            .and_then(|request| plz_registry(request, registry))
            .map(|request| request.release_argv()),
    }
    .map_err(map_mise)?;
    let args: Vec<OsString> = payload.into_iter().skip(1).collect();
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::ReleasePlz],
        OsStr::new("release-plz"),
        args,
    )
    .map_err(map_mise)?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Bind `--registry` on a `release-pr` request unless it is the default.
fn pr_registry(
    request: ReleasePrRequest,
    registry: Option<&str>,
) -> Result<ReleasePrRequest, velnor_actions_mise::MiseError> {
    match registry {
        Some(name) => request.with_registry(name),
        None => Ok(request),
    }
}

/// Bind `--registry` on a `release` request unless it is the default.
fn plz_registry(
    request: PlzRelease,
    registry: Option<&str>,
) -> Result<PlzRelease, velnor_actions_mise::MiseError> {
    match registry {
        Some(name) => request.with_registry(name),
        None => Ok(request),
    }
}

/// Map a Mise builder failure onto the contract error channel.
#[expect(clippy::needless_pass_by_value, reason = "map_err passes owned errors")]
fn map_mise(err: velnor_actions_mise::MiseError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: err.to_string(),
    }
}

/// Preparation: branch checkout, no gate, GitHub authority only.
fn preparation_job(
    parts: &JobParts<'_>,
    argv: Vec<String>,
    name: &str,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    Ok(ReleaseJobSpec {
        role: ReleaseRole::Preparation,
        display_name: "Release preparation".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: Vec::new(),
        condition: None,
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::Preparation),
        steps: vec![
            parts.policy_push.clone(),
            parts.setup.clone(),
            ambient_shell_step(name, argv, forge_env())?,
        ],
    })
}

/// Preflight: dry-run validation against the exact source.
fn preflight_job(
    parts: &JobParts<'_>,
    argv: Vec<String>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    Ok(ReleaseJobSpec {
        role: ReleaseRole::Preflight,
        display_name: "Release preflight".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: vec!["release-preparation".to_owned()],
        condition: None,
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::Preflight),
        steps: vec![
            parts.policy.clone(),
            parts.source.clone(),
            parts.setup.clone(),
            ambient_shell_step("Validate exact source", argv, forge_env())?,
        ],
    })
}

/// OIDC publish: gated, environment-pinned, zero registry-token material.
fn publish_job(
    parts: &JobParts<'_>,
    argv: Vec<String>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    Ok(ReleaseJobSpec {
        role: ReleaseRole::PublishOidc,
        display_name: "Release publish".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: vec!["release-preflight".to_owned()],
        condition: Some(parts.gate.to_owned()),
        environment: Some(parts.environment.to_owned()),
        permissions: JobPermissions::expected(ReleaseRole::PublishOidc),
        steps: vec![
            parts.policy.clone(),
            parts.source_push.clone(),
            parts.setup.clone(),
            ambient_shell_step("Publish release", argv, forge_env())?,
        ],
    })
}

/// Bootstrap publish: gated, environment-pinned, one token binding.
fn bootstrap_job(
    parts: &JobParts<'_>,
    argv: Vec<String>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    let mut env = forge_env();
    env.insert(
        BOOTSTRAP_TOKEN_ENV.to_owned(),
        BOOTSTRAP_SECRET_REF.to_owned(),
    );
    Ok(ReleaseJobSpec {
        role: ReleaseRole::PublishBootstrap,
        display_name: "Release bootstrap publish".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: vec!["release-preflight".to_owned()],
        condition: Some(parts.gate.to_owned()),
        environment: Some(parts.environment.to_owned()),
        permissions: JobPermissions::expected(ReleaseRole::PublishBootstrap),
        steps: vec![
            parts.policy.clone(),
            parts.source_push.clone(),
            parts.setup.clone(),
            ambient_shell_step("Publish first release", argv, env)?,
        ],
    })
}

/// Reconcile: independent revalidation after every publisher.
fn reconcile_job(
    parts: &JobParts<'_>,
    argv: Vec<String>,
    bootstrap: bool,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    let mut needs = vec!["release-publish".to_owned()];
    if bootstrap {
        needs.push("release-publish-bootstrap".to_owned());
    }
    Ok(ReleaseJobSpec {
        role: ReleaseRole::Reconcile,
        display_name: "Release reconcile".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs,
        condition: Some("always()".to_owned()),
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::Reconcile),
        steps: vec![
            parts.policy.clone(),
            parts.setup.clone(),
            ambient_shell_step("Verify published state", argv, forge_env())?,
        ],
    })
}
