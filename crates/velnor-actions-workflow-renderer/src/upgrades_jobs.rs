//! The four jobs of `upgrades.yml` and their shared step builders.

use velnor_actions_contract::{JobTimeout, StepRole};

use super::scripts::{GATE, PUBLISH, RELEASE, prepare_script, publish_check_script, verify_script};
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

/// One `run:` step under bash.
fn bash_step(name: &str, id: Option<&str>, step_env: &[(&str, &str)], run: String) -> Yaml {
    let mut fields = vec![("name".to_owned(), Yaml::str(name))];
    if let Some(id) = id {
        fields.push(("id".to_owned(), Yaml::str(id)));
    }
    fields.extend([
        ("shell".to_owned(), Yaml::str("bash")),
        ("env".to_owned(), env(step_env)),
        ("run".to_owned(), Yaml::str(run)),
    ]);
    Yaml::Map(fields)
}

/// Resolve and verify the newest stable Velnor before any repository code
/// runs; the read-only `github.token` lives only in this step.
fn velnor_release(spec: &UpgradesSpec) -> Yaml {
    bash_step(
        "Resolve and verify the latest Velnor release",
        None,
        &[
            ("GH_TOKEN", "${{ github.token }}"),
            ("VELNOR_UPGRADE_DIR", WORK_DIR),
            ("VELNOR_GH_VERSION", &spec.gh_version),
        ],
        RELEASE.to_owned(),
    )
}

fn download() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Download the upgrade patch")),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            env(&[("name", UPGRADES_ARTIFACT), ("path", WORK_DIR)]),
        ),
    ])
}

pub(super) fn prepare_job(spec: &UpgradesSpec) -> Result<Yaml, RenderError> {
    let tools = spec.generate_tools.join(" ");
    let bump = bash_step(
        "Bump every pin to the latest stable release",
        Some("bump"),
        &[
            ("MISE_MINIMUM_RELEASE_AGE", "0s"),
            ("MISE_YES", "1"),
            ("VELNOR_UPGRADE_DIR", WORK_DIR),
            ("VELNOR_CARGO_EDIT_VERSION", CARGO_EDIT_VERSION),
            ("VELNOR_CATALOG_BUMPER_SOURCE", CATALOG_BUMPER),
            ("VELNOR_GENERATE_TOOLS", &tools),
            ("VELNOR_VERIFY_RUST_SCRIPT", &spec.verify_rust_script),
        ],
        prepare_script(),
    );
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
                env(&[
                    ("changed", "${{ steps.bump.outputs.changed }}"),
                    ("failed", "${{ steps.bump.outputs.failed }}"),
                    ("failures", "${{ steps.bump.outputs.failures }}"),
                ]),
            ),
        ],
        &[("contents", "read")],
        vec![
            checkout(spec)?,
            setup(spec)?,
            install_gh(spec),
            install_generate_tools(spec),
            verify_jactionlint(spec),
            velnor_release(spec),
            bump,
            upload,
        ],
    )
}

/// Regenerate `.github/` from the default branch with the verified Velnor and
/// accept the patch only when prepare's `.github/` is exactly that.
pub(super) fn verify_job(spec: &UpgradesSpec) -> Result<Yaml, RenderError> {
    let tools = spec.generate_tools.join(" ");
    let verify = bash_step(
        "Regenerate .github/ and compare it with the patch",
        Some("verify"),
        &[
            ("VELNOR_UPGRADE_DIR", WORK_DIR),
            ("VELNOR_GENERATE_TOOLS", &tools),
            ("VELNOR_VERIFY_RUST_SCRIPT", &spec.verify_rust_script),
        ],
        verify_script(),
    );
    job(
        "Verify the upgrade patch",
        spec,
        30,
        vec![
            ("needs".to_owned(), Yaml::str("prepare")),
            (
                "if".to_owned(),
                Yaml::str("needs.prepare.outputs.changed == 'true'"),
            ),
            (
                "outputs".to_owned(),
                env(&[
                    ("tree", "${{ steps.verify.outputs.tree }}"),
                    ("patch_sha256", "${{ steps.verify.outputs.patch_sha256 }}"),
                    ("workflows", "${{ steps.verify.outputs.workflows }}"),
                ]),
            ),
        ],
        &[("contents", "read")],
        vec![
            checkout(spec)?,
            setup(spec)?,
            install_gh(spec),
            install_generate_tools(spec),
            verify_jactionlint(spec),
            download(),
            velnor_release(spec),
            verify,
        ],
    )
}

pub(super) fn publish_job(spec: &UpgradesSpec) -> Result<Yaml, RenderError> {
    let verified = [
        ("VELNOR_UPGRADE_DIR", WORK_DIR),
        ("VELNOR_VERIFIED_TREE", "${{ needs.verify.outputs.tree }}"),
        (
            "VELNOR_VERIFIED_PATCH_SHA256",
            "${{ needs.verify.outputs.patch_sha256 }}",
        ),
    ];
    let check = bash_step(
        "Check the patch is the verified upgrade",
        None,
        &verified,
        publish_check_script(),
    );
    let mut push_env = vec![("GH_TOKEN", "${{ steps.upgrade-token.outputs.token }}")];
    push_env.extend(verified);
    push_env.extend([
        ("VELNOR_UPGRADE_BRANCH", UPGRADES_BRANCH),
        (
            "VELNOR_DEFAULT_BRANCH",
            "${{ github.event.repository.default_branch }}",
        ),
        ("VELNOR_GH_VERSION", &spec.gh_version),
    ]);
    let push = bash_step(
        "Push the upgrade branch and open or update its pull request",
        None,
        &push_env,
        PUBLISH.to_owned(),
    );
    job(
        "Open or update the upgrade pull request",
        spec,
        10,
        vec![("needs".to_owned(), Yaml::str("verify"))],
        &[("contents", "read")],
        vec![
            checkout(spec)?,
            setup(spec)?,
            install_gh(spec),
            download(),
            check,
            mint(spec),
            push,
        ],
    )
}

/// App token for this repository only, minted after every check passed:
/// contents and pull requests, plus workflows only when the verified tree
/// changes `.github/workflows/` (an empty input requests nothing).
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
                (
                    "permission-workflows".to_owned(),
                    Yaml::str("${{ needs.verify.outputs.workflows }}"),
                ),
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
                ("VELNOR_SCOPE_FAILED", "${{ needs.prepare.outputs.failed }}"),
                (
                    "VELNOR_SCOPE_FAILURES",
                    "${{ needs.prepare.outputs.failures }}",
                ),
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

/// Install the validators `generate` executes, so the next step can check the
/// jactionlint executable before anything runs it. The later prerequisite
/// install in the regeneration script is then a no-op.
fn install_generate_tools(spec: &UpgradesSpec) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Install the pinned generate validators"),
        ),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "mise --no-config --no-env --no-hooks install {}",
                spec.generate_tools.join(" ")
            )),
        ),
    ])
}

fn verify_jactionlint(spec: &UpgradesSpec) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Verify jactionlint")),
        ("shell".to_owned(), Yaml::str("sh")),
        ("run".to_owned(), Yaml::str(spec.jactionlint_verify.clone())),
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
