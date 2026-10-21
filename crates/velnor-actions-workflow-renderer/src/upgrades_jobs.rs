//! The four jobs of `upgrades.yml` and their shared step builders.

use velnor_actions_contract::{JobTimeout, StepRole};

use super::scripts::{GATE, prepare_script, publish_script};
use super::{
    CARGO_EDIT_VERSION, CATALOG_BUMPER, ON_DEFAULT_BRANCH, UPGRADES_ARTIFACT, UPGRADES_BRANCH,
    UpgradesSpec, WORK_DIR,
};
use crate::{
    RenderError, mise_setup_step,
    steps::{self, CREATE_GITHUB_APP_TOKEN_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES},
    yaml::Yaml,
};

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

pub(super) fn prepare_job(spec: &UpgradesSpec) -> Result<Yaml, RenderError> {
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
        ("run".to_owned(), Yaml::str(prepare_script())),
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

pub(super) fn publish_job(spec: &UpgradesSpec) -> Result<Yaml, RenderError> {
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
        ("run".to_owned(), Yaml::str(publish_script())),
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

pub(super) fn gate_job(spec: &UpgradesSpec) -> Result<Yaml, RenderError> {
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
pub(super) fn env(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}
