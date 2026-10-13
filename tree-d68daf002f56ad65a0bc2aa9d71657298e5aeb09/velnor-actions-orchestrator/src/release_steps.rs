//! Release job assembly: pinned coordinator argv plus the role set.
//!
//! Every release-plz invocation runs through pinned Mise tools
//! (`rust` plus `release-plz`, exact versions from the compiled
//! catalog) with an explicit `--config` binding; no argv is invented
//! here — shapes come from the Mise release-plz builders. OIDC mode
//! assembles four jobs; bootstrap-token mode adds the fifth publish
//! job carrying the single token binding.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use velnor_actions_contract::Step;
use velnor_actions_contract::config::{ReleaseAuthentication, RustReleaseConfig};
use velnor_actions_mise::catalog::release_plz::{ReleasePrRequest, ReleaseRequest as PlzRelease};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_workflow_renderer::release_jobs::{ReleaseJobSpec, ReleaseRole};
use velnor_actions_workflow_renderer::release_permissions::JobPermissions;
use velnor_actions_workflow_renderer::release_tree::{
    RELEASE_BOOTSTRAP_CONFIG_PATH, RELEASE_CONFIG_PATH,
};
use velnor_actions_workflow_renderer::{MiseSetup, action_step, mise_setup_step, shell_step};

use crate::OrchestratorError;
use crate::utf8::strings_of;
use crate::workflow::CHECKOUT_USES;

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
    /// Approved source SHA every checkout pins.
    pub(crate) sha: &'a str,
    /// Plan registry for coordinator argv.
    pub(crate) registry: &'a str,
    /// Literal runner label.
    pub(crate) label: &'a str,
    /// Pinned Mise setup step inputs.
    pub(crate) mise: &'a MiseSetup,
    /// Pinned tool catalog for coordinator argv.
    pub(crate) catalog: &'a ToolCatalog,
}

/// One job's assembled head (checkout plus setup) and bindings.
struct JobParts<'a> {
    /// Exact-source credential-free checkout.
    checkout: Step,
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
/// Preparation runs `release-pr`, preflight and reconcile validate via
/// `release --dry-run`, and each publish job runs `release` against
/// its own generated config.
///
/// # Errors
///
/// Returns render or contract errors for invalid steps or argv.
pub(crate) fn assemble_jobs(
    inputs: &JobInputs<'_>,
) -> Result<BTreeMap<String, ReleaseJobSpec>, OrchestratorError> {
    let parts = JobParts {
        checkout: checkout_step(inputs.sha)?,
        setup: mise_setup_step(inputs.mise)?,
        label: inputs.label,
        gate: &inputs.gate,
        environment: &inputs.release.environment,
    };
    let manifest = inputs.release.manifest_path.as_str();
    let registry = inputs.registry;
    let catalog = inputs.catalog;
    let mut jobs = BTreeMap::new();
    let (prepare, name) = if inputs.release.release_pr {
        (Phase::PreparePr, "Prepare release PR")
    } else {
        (Phase::DryRun, "Validate release")
    };
    let argv = phase_argv(&prepare, RELEASE_CONFIG_PATH, manifest, registry, catalog)?;
    jobs.insert(
        "release-preparation".to_owned(),
        preparation_job(&parts, argv, name)?,
    );
    let argv = phase_argv(
        &Phase::DryRun,
        RELEASE_CONFIG_PATH,
        manifest,
        registry,
        catalog,
    )?;
    jobs.insert("release-preflight".to_owned(), preflight_job(&parts, argv)?);
    let argv = phase_argv(
        &Phase::Publish,
        RELEASE_CONFIG_PATH,
        manifest,
        registry,
        catalog,
    )?;
    jobs.insert("release-publish".to_owned(), publish_job(&parts, argv)?);
    let bootstrap = inputs.release.authentication == ReleaseAuthentication::BootstrapToken;
    if bootstrap {
        let argv = phase_argv(
            &Phase::Publish,
            RELEASE_BOOTSTRAP_CONFIG_PATH,
            manifest,
            registry,
            catalog,
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
        registry,
        catalog,
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
    registry: &str,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let path = PathBuf::from(config);
    let payload = match phase {
        Phase::PreparePr => ReleasePrRequest::new(path)
            .and_then(|request| request.with_manifest(PathBuf::from(manifest)))
            .and_then(|request| request.with_registry(registry))
            .map(|request| request.release_pr_argv()),
        Phase::Publish => PlzRelease::release(path)
            .and_then(|request| request.with_manifest(PathBuf::from(manifest)))
            .and_then(|request| request.with_registry(registry))
            .map(|request| request.release_argv()),
        Phase::DryRun => PlzRelease::dry_run(path)
            .and_then(|request| request.with_manifest(PathBuf::from(manifest)))
            .and_then(|request| request.with_registry(registry))
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

/// Map a Mise builder failure onto the contract error channel.
#[expect(clippy::needless_pass_by_value, reason = "map_err passes owned errors")]
fn map_mise(err: velnor_actions_mise::MiseError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: err.to_string(),
    }
}

/// Exact-source credential-free checkout shared by every release job.
///
/// # Errors
///
/// Returns render errors for invalid refs (ruled out: SHAs are validated).
fn checkout_step(sha: &str) -> Result<Step, OrchestratorError> {
    Ok(action_step(
        "Checkout",
        CHECKOUT_USES,
        BTreeMap::from([
            ("persist-credentials".to_owned(), "false".to_owned()),
            ("ref".to_owned(), sha.to_owned()),
        ]),
    )?)
}

/// Preparation: no needs, no gate, GitHub authority only.
fn preparation_job(
    parts: &JobParts<'_>,
    argv: Vec<String>,
    name: &str,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    Ok(ReleaseJobSpec {
        role: ReleaseRole::Preparation,
        display_name: "Release preparation".to_owned(),
        runs_on: parts.label.to_owned(),
        needs: Vec::new(),
        condition: None,
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::Preparation),
        steps: vec![
            parts.checkout.clone(),
            parts.setup.clone(),
            shell_step(name, argv, BTreeMap::new())?,
        ],
    })
}

/// Preflight: read-only validation after preparation.
fn preflight_job(
    parts: &JobParts<'_>,
    argv: Vec<String>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    Ok(ReleaseJobSpec {
        role: ReleaseRole::Preflight,
        display_name: "Release preflight".to_owned(),
        runs_on: parts.label.to_owned(),
        needs: vec!["release-preparation".to_owned()],
        condition: None,
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::Preflight),
        steps: vec![
            parts.checkout.clone(),
            parts.setup.clone(),
            shell_step("Validate exact source", argv, BTreeMap::new())?,
        ],
    })
}

/// OIDC publish: gated, environment-pinned, zero token material.
fn publish_job(
    parts: &JobParts<'_>,
    argv: Vec<String>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    Ok(ReleaseJobSpec {
        role: ReleaseRole::PublishOidc,
        display_name: "Release publish".to_owned(),
        runs_on: parts.label.to_owned(),
        needs: vec!["release-preflight".to_owned()],
        condition: Some(parts.gate.to_owned()),
        environment: Some(parts.environment.to_owned()),
        permissions: JobPermissions::expected(ReleaseRole::PublishOidc),
        steps: vec![
            parts.checkout.clone(),
            parts.setup.clone(),
            shell_step("Publish release", argv, BTreeMap::new())?,
        ],
    })
}

/// Bootstrap publish: gated, environment-pinned, one token binding.
fn bootstrap_job(
    parts: &JobParts<'_>,
    argv: Vec<String>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    let env = BTreeMap::from([(
        BOOTSTRAP_TOKEN_ENV.to_owned(),
        BOOTSTRAP_SECRET_REF.to_owned(),
    )]);
    Ok(ReleaseJobSpec {
        role: ReleaseRole::PublishBootstrap,
        display_name: "Release bootstrap publish".to_owned(),
        runs_on: parts.label.to_owned(),
        needs: vec!["release-preflight".to_owned()],
        condition: Some(parts.gate.to_owned()),
        environment: Some(parts.environment.to_owned()),
        permissions: JobPermissions::expected(ReleaseRole::PublishBootstrap),
        steps: vec![
            parts.checkout.clone(),
            parts.setup.clone(),
            shell_step("Publish first release", argv, env)?,
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
        needs,
        condition: Some("always()".to_owned()),
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::Reconcile),
        steps: vec![
            parts.checkout.clone(),
            parts.setup.clone(),
            shell_step("Verify published state", argv, BTreeMap::new())?,
        ],
    })
}
