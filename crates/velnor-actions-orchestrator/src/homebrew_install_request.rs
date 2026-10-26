// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rustix::fs::{self as unix_fs, Mode, OFlags};
use velnor_actions_contract::homebrew_spec::{
    HomebrewAuxiliaryTest, HomebrewPayloadRole, parse_generated_formula,
};
use velnor_actions_contract::package_release::PackageReleaseChannel;
use velnor_actions_mise::CheckDeadline;

use super::support::{
    current_host_target, expected_elf_for_asset, invalid, io_error, is_sha256, safe_component,
    safe_relative_path, sha256,
};
use super::{
    FORMULA_LIMIT, HOMEBREW_HOST_INSTALLED_MODE, HomebrewInstallRequest, auxiliary_installed_mode,
};
use crate::{OrchestratorError, exclusive_write, filesystem_owner::held_file::HeldFile};

pub(super) fn validate_request(
    request: &HomebrewInstallRequest<'_>,
) -> Result<(), OrchestratorError> {
    request
        .spec
        .validate()
        .map_err(|_| invalid("invalid_formula_spec"))?;
    let parsed_literals =
        parse_generated_formula(request.spec, request.projection, request.formula_bytes)
            .map_err(|_| invalid("formula_candidate_not_canonical"))?;
    if request.literals.is_disabled_stable_sentinel()
        || parsed_literals != *request.literals
        || !safe_component(request.literals.version())
        || request.projection.channel() == PackageReleaseChannel::Stable
            && request.literals.version() == "0.0.0"
        || request.formula_bytes.is_empty()
        || request.formula_bytes.len() > FORMULA_LIMIT
        || !is_sha256(request.formula_sha256)
        || sha256(request.formula_bytes) != request.formula_sha256
    {
        return Err(invalid("invalid_formula_candidate"));
    }
    let formula = request
        .spec
        .channels
        .iter()
        .find(|channel| channel.channel == request.projection.channel())
        .ok_or_else(|| invalid("formula_channel_missing"))?;
    if !safe_component(&formula.formula)
        || request.literals.assets().len() != request.projection.selected_payloads().len()
    {
        return Err(invalid("formula_identity_invalid"));
    }
    validate_file_inventory(request)?;
    for path in [
        &request.paths.home,
        &request.paths.temp,
        &request.paths.staging,
    ] {
        if !path.is_absolute() {
            return Err(invalid("install_path_not_absolute"));
        }
    }
    Ok(())
}

pub(super) fn validate_file_inventory(
    request: &HomebrewInstallRequest<'_>,
) -> Result<(), OrchestratorError> {
    let host_target = current_host_target()?;
    validate_file_inventory_for_host(request, host_target)
}

fn validate_file_inventory_for_host(
    request: &HomebrewInstallRequest<'_>,
    host_target: &str,
) -> Result<(), OrchestratorError> {
    if !request
        .spec
        .host
        .targets
        .iter()
        .any(|target| target == host_target)
    {
        return Err(invalid("host_target_not_declared"));
    }
    let host = request
        .projection
        .selected_payloads()
        .iter()
        .find(|payload| {
            payload.role() == HomebrewPayloadRole::Host && payload.target() == host_target
        })
        .ok_or_else(|| invalid("host_payload_not_selected"))?;
    let host_asset = host.asset_name();
    let mut expected = BTreeMap::<(String, String), (String, u32, HomebrewAuxiliaryTest)>::new();
    for bin in &request.spec.host.bins {
        expected.insert(
            (host_asset.to_owned(), bin.clone()),
            (
                format!("bin/{bin}"),
                HOMEBREW_HOST_INSTALLED_MODE,
                HomebrewAuxiliaryTest::None,
            ),
        );
    }

    let architecture = host_target.split('-').next().unwrap_or_default();
    for auxiliary in &request.spec.auxiliaries {
        for payload in request
            .projection
            .selected_payloads()
            .iter()
            .filter(|payload| {
                payload.role() == HomebrewPayloadRole::Auxiliary
                    && payload.product() == auxiliary.product
                    && payload.target().starts_with(architecture)
                    && payload.target().contains("linux")
            })
        {
            for file in auxiliary
                .files
                .iter()
                .filter(|file| file.target == payload.target())
            {
                let mode = auxiliary_installed_mode(file.mode);
                expected.insert(
                    (payload.asset_name().to_owned(), file.member.clone()),
                    (format!("libexec/{}", file.destination), mode, file.test),
                );
            }
        }
    }
    validate_observed_files(request, &expected)
}

fn validate_observed_files(
    request: &HomebrewInstallRequest<'_>,
    expected: &BTreeMap<(String, String), (String, u32, HomebrewAuxiliaryTest)>,
) -> Result<(), OrchestratorError> {
    if expected.is_empty() || expected.len() != request.files.len() {
        return Err(invalid("installed_file_inventory_mismatch"));
    }
    let mut observed = BTreeSet::new();
    for file in request.files {
        let key = (file.asset.clone(), file.member.clone());
        let Some((destination, mode, test)) = expected.get(&key) else {
            return Err(invalid("installed_file_not_selected"));
        };
        if !observed.insert(key)
            || !safe_relative_path(&file.destination)
            || file.destination != Path::new(destination)
            || file.mode != *mode
            || file.test != *test
            || !is_sha256(&file.sha256)
            || (file.test == HomebrewAuxiliaryTest::LinuxElf) != file.elf.is_some()
            || file
                .elf
                .is_some_and(|facts| expected_elf_for_asset(request, &file.asset) != Some(facts))
        {
            return Err(invalid("installed_file_policy_mismatch"));
        }
    }
    Ok(())
}

pub(super) fn formula_name(
    request: &HomebrewInstallRequest<'_>,
) -> Result<String, OrchestratorError> {
    request
        .spec
        .channels
        .iter()
        .find(|channel| channel.channel == request.projection.channel())
        .map(|channel| channel.formula.clone())
        .ok_or_else(|| invalid("formula_channel_missing"))
}

pub(super) fn write_candidate_formula(
    request: &HomebrewInstallRequest<'_>,
    formula: &str,
) -> Result<PathBuf, OrchestratorError> {
    let path = request.paths.staging.join(format!("{formula}.rb"));
    write_formula_file(
        &path,
        request.formula_bytes,
        request.formula_sha256,
        request.deadline,
    )
}

pub(super) fn write_formula_file(
    path: &Path,
    formula_bytes: &[u8],
    formula_sha256: &str,
    deadline: CheckDeadline,
) -> Result<PathBuf, OrchestratorError> {
    exclusive_write::write_exclusive_until(
        path,
        formula_bytes,
        "homebrew_candidate_formula",
        || {
            deadline
                .remaining()
                .map(|_| ())
                .map_err(|_| invalid("deadline"))
        },
    )?;
    let fd = unix_fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| invalid("candidate_formula_open_failed"))?;
    let file = File::from(fd);
    let before = file.metadata().map_err(io_error)?;
    if !before.is_file()
        || before.uid() != crate::filesystem_owner::effective_uid()
        || before.len() != formula_bytes.len() as u64
    {
        return Err(invalid("candidate_formula_identity_invalid"));
    }
    file.set_permissions(fs::Permissions::from_mode(0o644))
        .map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    let named = fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o777 != 0o644
        || metadata.dev() != named.dev()
        || metadata.ino() != named.ino()
        || metadata.len() != formula_bytes.len() as u64
    {
        return Err(invalid("candidate_formula_mode_invalid"));
    }
    let held = HeldFile::new(path, formula_sha256, formula_bytes.len() as u64, deadline)?;
    held.verify(deadline)?;
    Ok(path.to_path_buf())
}
