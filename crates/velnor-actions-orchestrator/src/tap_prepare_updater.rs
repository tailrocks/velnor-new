use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use velnor_actions_contract::{PackageReleasePlan, VerifiedPackageRelease, canonical_json_bytes};
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck};

use super::{OrchestratorError, TASK_OUTPUT_LIMIT, internal, sha256};
use crate::check_runtime::preparation::prepare_check;

pub(super) fn prepare_receipt_handoff(
    temp: &Path,
    bytes: &[u8],
) -> Result<PathBuf, OrchestratorError> {
    let directory = temp.join("tap-release-handoff");
    crate::exclusive_write::create_dir_no_symlink(temp, &directory)?;
    let path = directory.join("release.json");
    crate::exclusive_write::write_exclusive(&path, bytes, "tap_release_handoff")?;
    let mut permissions = fs::metadata(&path)
        .map_err(|_| internal("tap_release_handoff_metadata"))?
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions)
        .map_err(|_| internal("tap_release_handoff_permissions"))?;
    Ok(path)
}

pub(super) fn prepare_plan_handoff(
    temp: &Path,
    plan: &PackageReleasePlan,
) -> Result<PathBuf, OrchestratorError> {
    let directory = temp.join("tap-release-handoff");
    crate::exclusive_write::create_dir_no_symlink(temp, &directory)?;
    let path = directory.join("plan.json");
    let bytes = plan
        .canonical_bytes()
        .map_err(|_| internal("tap_plan_handoff_encode"))?;
    crate::exclusive_write::write_exclusive(&path, &bytes, "tap_plan_handoff")?;
    let mut permissions = fs::metadata(&path)
        .map_err(|_| internal("tap_plan_handoff_metadata"))?
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions)
        .map_err(|_| internal("tap_plan_handoff_permissions"))?;
    Ok(path)
}

pub(super) struct UpdaterRun<'a> {
    pub(super) root: &'a Path,
    pub(super) temp: &'a Path,
    pub(super) discovered: &'a DiscoveredCheck,
    pub(super) deadline: CheckDeadline,
    pub(super) receipt_path: &'a Path,
    pub(super) plan_path: &'a Path,
    pub(super) base_sha: &'a str,
    pub(super) formula: &'a str,
    pub(super) release: &'a VerifiedPackageRelease,
}

pub(super) struct CapturedUpdaterRun<'a> {
    pub(super) release: &'a VerifiedPackageRelease,
    pub(super) plan: &'a PackageReleasePlan,
    pub(super) formula_path: &'a str,
    pub(super) temp: &'a Path,
    pub(super) captured: &'a super::CapturedTapMain,
    pub(super) discovered: &'a DiscoveredCheck,
    pub(super) deadline: CheckDeadline,
}

pub(super) fn run_captured_updater(run: &CapturedUpdaterRun<'_>) -> Result<(), OrchestratorError> {
    let receipt_bytes =
        canonical_json_bytes(&run.release.receipt).map_err(|_| internal("tap_receipt_encode"))?;
    if run
        .release
        .receipt
        .canonical_digest()
        .map_err(|_| internal("tap_receipt_digest"))?
        != run.release.receipt_digest
    {
        return Err(internal("tap_receipt_digest_mismatch"));
    }
    let receipt_path = prepare_receipt_handoff(run.temp, &receipt_bytes)?;
    let plan_path = prepare_plan_handoff(run.temp, run.plan)?;
    run_updater(&UpdaterRun {
        root: &run.captured.checkout,
        temp: run.temp,
        discovered: run.discovered,
        deadline: run.deadline,
        receipt_path: &receipt_path,
        plan_path: &plan_path,
        base_sha: &run.captured.commit_sha,
        formula: run.formula_path,
        release: run.release,
    })?;
    if fs::read(&receipt_path).map_err(|_| internal("tap_receipt_handoff_read"))? != receipt_bytes {
        return Err(internal("tap_receipt_handoff_changed"));
    }
    if fs::read(&plan_path).map_err(|_| internal("tap_plan_handoff_read"))?
        != run
            .plan
            .canonical_bytes()
            .map_err(|_| internal("tap_plan_handoff_encode"))?
    {
        return Err(internal("tap_plan_handoff_changed"));
    }
    Ok(())
}

pub(super) fn run_updater(run: &UpdaterRun<'_>) -> Result<(), OrchestratorError> {
    let UpdaterRun {
        root,
        temp,
        discovered,
        deadline,
        receipt_path,
        plan_path,
        base_sha,
        formula,
        release,
    } = run;
    if discovered.check.runner.platform.os() != std::env::consts::OS
        || discovered.check.runner.platform.arch() != std::env::consts::ARCH
    {
        return Err(internal("tap_updater_host_platform"));
    }
    let owned = prepare_check(root, temp, discovered, *deadline)?;
    let handoff = updater_environment(base_sha, formula, receipt_path, plan_path, release)?;
    let pairs: Vec<(OsString, OsString)> = handoff
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect();
    let command = owned
        .command(*deadline)
        .map_err(|error| internal(&error.to_string()))?
        .with_env(&pairs)
        .map_err(|error| internal(&error.to_string()))?;
    command
        .run_until(TASK_OUTPUT_LIMIT, *deadline)
        .map_err(|error| internal(&error.to_string()))?
        .require_success("tap-updater")
        .map_err(|error| internal(&error.to_string()))?;
    Ok(())
}

fn updater_environment(
    base_sha: &str,
    formula: &str,
    receipt_path: &Path,
    plan_path: &Path,
    release: &VerifiedPackageRelease,
) -> Result<[(&'static str, String); 9], OrchestratorError> {
    Ok([
        ("VELNOR_TAP_BASE_SHA", base_sha.to_owned()),
        ("VELNOR_TAP_FORMULA_PATH", formula.to_owned()),
        (
            "VELNOR_TAP_RELEASE_RECEIPT",
            receipt_path
                .to_str()
                .ok_or_else(|| internal("tap_receipt_path_utf8"))?
                .to_owned(),
        ),
        (
            "VELNOR_TAP_RELEASE_RECEIPT_SHA256",
            sha256(
                &canonical_json_bytes(&release.receipt)
                    .map_err(|_| internal("tap_receipt_encode"))?,
            ),
        ),
        (
            "VELNOR_TAP_RELEASE_PLAN",
            plan_path
                .to_str()
                .ok_or_else(|| internal("tap_plan_path_utf8"))?
                .to_owned(),
        ),
        (
            "VELNOR_TAP_RELEASE_PLAN_SHA256",
            sha256(&fs::read(plan_path).map_err(|_| internal("tap_plan_handoff_read"))?),
        ),
        (
            "VELNOR_TAP_RELEASE_VERSION",
            release.receipt.version.value.clone(),
        ),
        ("VELNOR_TAP_RELEASE_TAG", release.receipt.tag.clone()),
        (
            "VELNOR_TAP_SOURCE_SHA",
            release.receipt.source.commit_sha.clone(),
        ),
    ])
}
