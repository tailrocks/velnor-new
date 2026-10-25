use super::inventory::{validate_brew_sbom, verify_keg_file_inventory, verify_payload_directories};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use velnor_actions_contract::homebrew_spec::{HomebrewAuxiliaryTest, HomebrewPayloadRole};

use super::request::formula_name;
use super::support::{
    current_host_target, digest_field, elf_facts, epoch_seconds, hex, invalid, io_error,
    is_supported_host_macho, read_regular_nofollow, run_command, safe_relative_path, sha256,
    snapshot, verify_trusted_path,
};
use super::{
    ElfFacts, FILE_LIMIT, FORMULA_LIMIT, HOMEBREW_CANDIDATE_TAP, HOMEBREW_VERSION,
    HomebrewInstallFile, HomebrewInstallRequest, HomebrewInstalledFileEvidence, KegState,
};
use crate::{OrchestratorError, homebrew::command::HomebrewCommand};

pub(super) fn inspect_keg(
    request: &HomebrewInstallRequest<'_>,
    keg: &Path,
    candidate_recipe: &Path,
    install_started: u64,
) -> Result<KegState, OrchestratorError> {
    verify_trusted_path(keg, true)?;
    let keg_identity = snapshot(keg)?;
    let metadata = inspect_formula_metadata(request, keg, candidate_recipe, install_started)?;
    let (payload_sha256, installed_files, expected_paths) = verify_installed_payload(request, keg)?;
    verify_payload_directories(keg, &expected_paths)?;
    verify_keg_file_inventory(keg, &metadata.formula, &expected_paths)?;
    let version_output_sha256 = verify_host_version_outputs(request, keg)?;
    let install_time = receipt_time(&metadata.receipt)?;
    if snapshot(keg)? != keg_identity {
        return Err(invalid("keg_identity_changed"));
    }
    Ok(KegState {
        receipt: metadata.receipt,
        receipt_sha256: metadata.receipt_sha256,
        sbom_sha256: metadata.sbom_sha256,
        sbom: metadata.sbom,
        recipe_sha256: metadata.recipe_sha256,
        payload_sha256,
        installed_files,
        version_output_sha256,
        install_time,
        keg_identity,
    })
}

struct FormulaMetadata {
    formula: String,
    receipt: Vec<u8>,
    receipt_sha256: String,
    sbom: Vec<u8>,
    sbom_sha256: String,
    recipe_sha256: String,
}

fn inspect_formula_metadata(
    request: &HomebrewInstallRequest<'_>,
    keg: &Path,
    candidate_recipe: &Path,
    install_started: u64,
) -> Result<FormulaMetadata, OrchestratorError> {
    let formula = formula_name(request)?;
    let receipt_path = keg.join("INSTALL_RECEIPT.json");
    let receipt = read_regular_nofollow(&receipt_path, 4 * 1024 * 1024)?;
    validate_receipt(
        &receipt,
        candidate_recipe,
        request.literals.version(),
        install_started,
    )?;
    let receipt_sha256 = sha256(&receipt);

    let sbom_path = keg.join("sbom.spdx.json");
    let sbom = read_regular_nofollow(&sbom_path, 1024 * 1024)?;
    let host_target = current_host_target()?;
    let host_payload = request
        .projection
        .selected_payloads()
        .iter()
        .find(|payload| {
            payload.role() == HomebrewPayloadRole::Host && payload.target() == host_target
        })
        .ok_or_else(|| invalid("installed_brew_sbom_host_asset"))?;
    let host_asset = request
        .literals
        .assets()
        .iter()
        .find(|asset| asset.name() == host_payload.asset_name())
        .ok_or_else(|| invalid("installed_brew_sbom_host_asset"))?;
    validate_brew_sbom(
        &sbom,
        &formula,
        request.literals.version(),
        host_asset.name(),
        host_asset.sha256(),
        host_asset.url(),
    )?;
    let sbom_sha256 = sha256(&sbom);

    let archived_recipe = keg.join(".brew").join(format!("{formula}.rb"));
    let recipe_bytes = read_regular_nofollow(&archived_recipe, FORMULA_LIMIT)?;
    let recipe_sha256 = sha256(&recipe_bytes);
    if recipe_bytes != request.formula_bytes || recipe_sha256 != request.formula_sha256 {
        return Err(invalid("archived_formula_changed"));
    }
    let brew_entries = fs::read_dir(keg.join(".brew"))
        .map_err(io_error)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(io_error)?;
    let expected_brew_entries = [OsString::from(format!("{formula}.rb"))]
        .into_iter()
        .collect::<BTreeSet<_>>();
    if brew_entries != expected_brew_entries {
        return Err(invalid("installed_tab_inventory_mismatch"));
    }

    Ok(FormulaMetadata {
        formula,
        receipt,
        receipt_sha256,
        sbom,
        sbom_sha256,
        recipe_sha256,
    })
}

fn verify_installed_payload(
    request: &HomebrewInstallRequest<'_>,
    keg: &Path,
) -> Result<
    (
        String,
        Vec<HomebrewInstalledFileEvidence>,
        BTreeSet<PathBuf>,
    ),
    OrchestratorError,
> {
    let mut digest = Sha256::new();
    let mut expected = BTreeSet::new();
    let mut files = Vec::with_capacity(request.files.len());
    for file in request.files {
        let installed = inspect_payload_file(file, keg, &mut digest, &mut expected)?;
        files.push(installed);
    }
    Ok((hex(&digest.finalize()), files, expected))
}

fn inspect_payload_file(
    file: &HomebrewInstallFile,
    keg: &Path,
    digest: &mut Sha256,
    expected: &mut BTreeSet<PathBuf>,
) -> Result<HomebrewInstalledFileEvidence, OrchestratorError> {
    if !expected.insert(file.destination.clone()) {
        return Err(invalid("duplicate_installed_destination"));
    }
    let path = keg.join(&file.destination);
    let file_limit =
        usize::try_from(FILE_LIMIT).map_err(|_| invalid("installed_payload_limit_overflow"))?;
    let bytes = read_regular_nofollow(&path, file_limit)?;
    let mode = fs::symlink_metadata(&path)
        .map_err(io_error)?
        .permissions()
        .mode()
        & 0o777;
    let hash = sha256(&bytes);
    if mode != file.mode || hash != file.sha256 {
        return Err(invalid("installed_payload_mismatch"));
    }
    validate_payload_format(file, &bytes)?;
    for field in [
        file.asset.as_bytes(),
        file.member.as_bytes(),
        file.destination.to_string_lossy().as_bytes(),
        hash.as_bytes(),
        format!("{mode:o}").as_bytes(),
    ] {
        digest_field(digest, field);
    }
    Ok(HomebrewInstalledFileEvidence {
        asset: file.asset.clone(),
        member: file.member.clone(),
        destination: file.destination.clone(),
        sha256: hash,
        mode,
        test: file.test,
        elf: observed_elf(file, &bytes)?,
    })
}

fn validate_payload_format(
    file: &HomebrewInstallFile,
    bytes: &[u8],
) -> Result<(), OrchestratorError> {
    if file.destination.starts_with("bin/") && !is_supported_host_macho(bytes) {
        return Err(invalid("installed_host_binary_macho_mismatch"));
    }
    let elf = observed_elf(file, bytes)?;
    if file.elf.is_some_and(|expected| Some(expected) != elf) {
        return Err(invalid("installed_elf_header_mismatch"));
    }
    Ok(())
}

fn observed_elf(
    file: &HomebrewInstallFile,
    bytes: &[u8],
) -> Result<Option<ElfFacts>, OrchestratorError> {
    if file.test == HomebrewAuxiliaryTest::LinuxElf {
        elf_facts(bytes).map(Some)
    } else {
        Ok(None)
    }
}

fn verify_host_version_outputs(
    request: &HomebrewInstallRequest<'_>,
    keg: &Path,
) -> Result<String, OrchestratorError> {
    let mut digest = Sha256::new();
    for bin in &request.spec.host.bins {
        let path = keg.join("bin").join(bin);
        let output = run_command(
            HomebrewCommand::InstalledBinaryVersion { executable: path },
            &request.paths.home,
            &request.paths.temp,
            request.deadline,
        )?;
        let text = String::from_utf8(output.stdout)
            .map_err(|_| invalid("installed_binary_version_not_utf8"))?;
        if !text.contains(request.literals.version()) {
            return Err(invalid("installed_binary_version_mismatch"));
        }
        digest_field(&mut digest, bin.as_bytes());
        digest_field(&mut digest, text.as_bytes());
    }
    Ok(hex(&digest.finalize()))
}

pub(super) fn validate_receipt(
    bytes: &[u8],
    candidate_recipe: &Path,
    version: &str,
    install_started: u64,
) -> Result<(), OrchestratorError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| invalid("install_receipt_not_json"))?;
    let source = value
        .get("source")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| invalid("install_receipt_source_missing"))?;
    let source_path = source
        .get("path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("install_receipt_source_path_missing"))?;
    let source_spec = source
        .get("spec")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("install_receipt_source_spec_missing"))?;
    let source_tap = source
        .get("tap")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("install_receipt_source_tap_missing"))?;
    let source_tap_git_head_is_null = source
        .get("tap_git_head")
        .is_some_and(serde_json::Value::is_null);
    let source_versions = source
        .get("versions")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| invalid("install_receipt_versions_missing"))?;
    let stable_version = source_versions
        .get("stable")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("install_receipt_stable_version_missing"))?;
    let time = value
        .get("time")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| invalid("install_receipt_time_missing"))?;
    let brew_version = value
        .get("homebrew_version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("install_receipt_brew_version_missing"))?;
    let installed_on_request = value
        .get("installed_on_request")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| invalid("install_receipt_request_state_missing"))?;
    let poured_from_bottle = value
        .get("poured_from_bottle")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| invalid("install_receipt_bottle_state_missing"))?;
    validate_receipt_changed_files(value.get("changed_files"))?;
    let now = epoch_seconds()?;
    if Path::new(source_path) != candidate_recipe
        || source_tap != HOMEBREW_CANDIDATE_TAP
        || !source_tap_git_head_is_null
        || source_spec != "stable"
        || stable_version != version
        || brew_version != HOMEBREW_VERSION.trim_start_matches("Homebrew ")
        || !installed_on_request
        || poured_from_bottle
        || time < install_started.saturating_sub(2)
        || time > now.saturating_add(5)
    {
        return Err(invalid("install_receipt_source_mismatch"));
    }
    if !version.split('.').all(|part| !part.is_empty()) {
        return Err(invalid("install_receipt_version_invalid"));
    }
    Ok(())
}

fn validate_receipt_changed_files(
    changed_files: Option<&serde_json::Value>,
) -> Result<(), OrchestratorError> {
    match changed_files {
        Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::Array(paths)) => {
            let raw = paths
                .iter()
                .map(|path| {
                    path.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| invalid("install_receipt_file_inventory_invalid"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if raw.iter().collect::<BTreeSet<_>>().len() != raw.len()
                || raw.iter().any(|path| !safe_relative_path(Path::new(path)))
            {
                return Err(invalid("install_receipt_file_inventory_invalid"));
            }
        }
        _ => return Err(invalid("install_receipt_file_inventory_missing")),
    }
    Ok(())
}

fn receipt_time(bytes: &[u8]) -> Result<u64, OrchestratorError> {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|value| value.get("time").and_then(serde_json::Value::as_u64))
        .ok_or_else(|| invalid("install_receipt_time_missing"))
}
