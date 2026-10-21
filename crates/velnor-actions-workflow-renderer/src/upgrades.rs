//! Opt-in daily upgrade workflow (`upgrades.yml`, `[upgrades]`).
//!
//! Three jobs, each with the least authority it needs:
//!
//! - `prepare` (`contents: read`, no credential): bumps Velnor, every Mise
//!   tool (Rust last, so a Rust release the selected Velnor cannot verify yet
//!   rolls back alone), Cargo, Bun and Gradle catalog dependencies to the
//!   newest stable release with `MISE_MINIMUM_RELEASE_AGE=0s`, regenerates
//!   `.github/` with the selected Velnor, and uploads a path-filtered patch.
//! - `publish` (`contents: read`): re-checks the patch paths, mints a GitHub
//!   App token for this repository only, fast-forwards `velnor/upgrades`
//!   (never a force push) and opens or updates the one upgrade pull request.
//!   The token never coexists with repository code.
//! - `freshness` (`contents: read`, `pull-requests: read`): fails while any
//!   pin lags the latest stable release or the pull request has been open
//!   longer than `stuck_days`. It runs only on this schedule, so it is never
//!   a pull-request check.

use velnor_actions_contract::config::{UPGRADES_WORKFLOW_PATH, UpgradesConfig};
use velnor_actions_contract::{JobTimeout, StepRole};

use crate::{
    MiseSetup, RenderError, guard, marker, mise_setup_step,
    render::RenderedFile,
    steps::{self, CREATE_GITHUB_APP_TOKEN_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES},
    yaml::Yaml,
};

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

const PREPARE: &str = include_str!("upgrades_prepare.sh");
const PUBLISH: &str = include_str!("upgrades_publish.sh");
const GATE: &str = include_str!("upgrades_gate.sh");
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
        Ok(())
    }
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

fn job(
    name: &str,
    spec: &UpgradesSpec,
    minutes: u16,
    head: Vec<(String, Yaml)>,
    perms: &[(&str, &str)],
    steps: Vec<Yaml>,
) -> Result<Yaml, RenderError> {
    let timeout = JobTimeout::new(minutes).map_err(RenderError::Contract)?;
    let mut fields = vec![("name".to_owned(), Yaml::str(name))];
    fields.extend(head);
    fields.extend([
        ("runs-on".to_owned(), Yaml::str(spec.runs_on.clone())),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(timeout.minutes())),
        ),
        ("permissions".to_owned(), env(perms)),
        ("steps".to_owned(), Yaml::Seq(steps)),
    ]);
    Ok(Yaml::Map(fields))
}

fn prepare_job(spec: &UpgradesSpec) -> Result<Yaml, RenderError> {
    let bump = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Bump every pin to the latest stable release"),
        ),
        ("id".to_owned(), Yaml::str("bump")),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            env(&[
                ("MISE_MINIMUM_RELEASE_AGE", "0s"),
                ("MISE_YES", "1"),
                ("VELNOR_UPGRADE_DIR", WORK_DIR),
                ("VELNOR_CARGO_EDIT_VERSION", CARGO_EDIT_VERSION),
                ("VELNOR_CATALOG_BUMPER_SOURCE", CATALOG_BUMPER),
            ]),
        ),
        ("run".to_owned(), Yaml::str(PREPARE)),
    ]);
    let upload = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Upload the upgrade patch")),
        (
            "if".to_owned(),
            Yaml::str("steps.bump.outputs.changed == 'true'"),
        ),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            env(&[
                ("name", UPGRADES_ARTIFACT),
                ("path", &format!("{WORK_DIR}/out")),
                ("retention-days", "3"),
                ("if-no-files-found", "error"),
            ]),
        ),
    ]);
    job(
        "Prepare upgrade",
        spec,
        60,
        vec![
            ("if".to_owned(), Yaml::str(ON_DEFAULT_BRANCH)),
            (
                "outputs".to_owned(),
                env(&[("changed", "${{ steps.bump.outputs.changed }}")]),
            ),
        ],
        &[("contents", "read")],
        vec![checkout(spec)?, setup(spec)?, bump, upload],
    )
}

fn publish_job(spec: &UpgradesSpec) -> Result<Yaml, RenderError> {
    let download = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Download the upgrade patch")),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            env(&[("name", UPGRADES_ARTIFACT), ("path", WORK_DIR)]),
        ),
    ]);
    let push = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Push the upgrade branch and open or update its pull request"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            env(&[
                ("GH_TOKEN", "${{ steps.upgrade-token.outputs.token }}"),
                ("VELNOR_UPGRADE_DIR", WORK_DIR),
                ("VELNOR_UPGRADE_BRANCH", UPGRADES_BRANCH),
                (
                    "VELNOR_DEFAULT_BRANCH",
                    "${{ github.event.repository.default_branch }}",
                ),
                ("VELNOR_GH_VERSION", &spec.gh_version),
            ]),
        ),
        ("run".to_owned(), Yaml::str(PUBLISH)),
    ]);
    job(
        "Open or update the upgrade pull request",
        spec,
        10,
        vec![
            ("needs".to_owned(), Yaml::str("prepare")),
            (
                "if".to_owned(),
                Yaml::str("needs.prepare.outputs.changed == 'true'"),
            ),
        ],
        &[("contents", "read")],
        vec![
            checkout(spec)?,
            setup(spec)?,
            install_gh(spec),
            download,
            mint(spec),
            push,
        ],
    )
}

/// App token for this repository only: contents, pull requests, workflows.
fn mint(spec: &UpgradesSpec) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Mint a short-lived token for this repository only"),
        ),
        ("id".to_owned(), Yaml::str("upgrade-token")),
        ("uses".to_owned(), Yaml::str(CREATE_GITHUB_APP_TOKEN_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                (
                    "client-id".to_owned(),
                    Yaml::str(format!("${{{{ vars.{} }}}}", spec.config.app_id_variable)),
                ),
                (
                    "private-key".to_owned(),
                    Yaml::str(format!(
                        "${{{{ secrets.{} }}}}",
                        spec.config.app_private_key_secret
                    )),
                ),
                (
                    "owner".to_owned(),
                    Yaml::str("${{ github.repository_owner }}"),
                ),
                (
                    "repositories".to_owned(),
                    Yaml::str("${{ github.event.repository.name }}"),
                ),
                ("permission-contents".to_owned(), Yaml::str("write")),
                ("permission-pull-requests".to_owned(), Yaml::str("write")),
                // GitHub rejects an App push that changes `.github/workflows/`
                // without it; regenerating `.github/` is the job's purpose.
                ("permission-workflows".to_owned(), Yaml::str("write")),
                ("skip-token-revoke".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ])
}

fn gate_job(spec: &UpgradesSpec) -> Result<Yaml, RenderError> {
    let stuck = spec.config.stuck_days.to_string();
    let gate = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Fail while a pin lags or the upgrade pull request is stuck"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            env(&[
                ("GH_TOKEN", "${{ github.token }}"),
                ("VELNOR_PREPARE_RESULT", "${{ needs.prepare.result }}"),
                ("VELNOR_STALE", "${{ needs.prepare.outputs.changed }}"),
                ("VELNOR_STUCK_DAYS", &stuck),
                ("VELNOR_UPGRADE_BRANCH", UPGRADES_BRANCH),
                ("VELNOR_GH_VERSION", &spec.gh_version),
            ]),
        ),
        ("run".to_owned(), Yaml::str(GATE)),
    ]);
    job(
        "Freshness gate",
        spec,
        5,
        vec![
            ("needs".to_owned(), Yaml::str("prepare")),
            (
                "if".to_owned(),
                Yaml::str(format!("${{{{ always() && {ON_DEFAULT_BRANCH} }}}}")),
            ),
        ],
        &[("contents", "read"), ("pull-requests", "read")],
        vec![setup(spec)?, install_gh(spec), gate],
    )
}

fn checkout(spec: &UpgradesSpec) -> Result<Yaml, RenderError> {
    let mut step = steps::action_step(
        "Checkout",
        &spec.checkout_uses,
        std::collections::BTreeMap::from([
            ("persist-credentials".to_owned(), "false".to_owned()),
            ("fetch-depth".to_owned(), "1".to_owned()),
        ]),
    )?;
    step.role = Some(StepRole::Checkout);
    crate::steps_plain::plain_step_to_yaml(&step)
}

fn setup(spec: &UpgradesSpec) -> Result<Yaml, RenderError> {
    crate::steps_plain::plain_step_to_yaml(&mise_setup_step(&spec.mise_setup)?)
}

fn install_gh(spec: &UpgradesSpec) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Install the pinned GitHub CLI"),
        ),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "mise --no-config --no-env --no-hooks install gh@{}",
                spec.gh_version
            )),
        ),
    ])
}

/// String map (`env:`, `with:`, `permissions:`, `outputs:`).
fn env(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

#[cfg(test)]
#[path = "upgrades_tests.rs"]
mod tests;
