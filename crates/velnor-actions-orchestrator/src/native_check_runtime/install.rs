use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::Instant,
};

use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    ObligationDecision, config::NativeHomebrewOperation, package_release::PackageReleaseChannel,
};
use velnor_actions_mise::CheckDeadline;

use super::{
    NativeHomebrewHostedProfileEvidence, NativeHomebrewInstallEvidence,
    VerifiedNativeHomebrewInstall,
};
use crate::OrchestratorError;

#[path = "install/release.rs"]
mod release;
use release::{InstallSource, load_install_source};
#[derive(Clone, Copy)]
pub(crate) struct InstallRequest<'a> {
    pub(crate) root: &'a Path,
    pub(crate) runner_temp: &'a Path,
    pub(crate) channel: PackageReleaseChannel,
    pub(crate) identity:
        &'a crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
    pub(crate) event: velnor_actions_contract::WorkflowEvent,
    pub(crate) deadline: CheckDeadline,
    pub(crate) plan: &'a velnor_actions_contract::Plan,
    pub(crate) entry: &'a velnor_actions_contract::MatrixEntry,
    pub(crate) obligation: &'a velnor_actions_contract::PlanObligation,
    pub(crate) entry_digest: &'a str,
    pub(crate) task_id: &'a str,
    pub(crate) check_id: &'a str,
}

pub(crate) fn install_formula(
    request: InstallRequest<'_>,
) -> Result<
    (
        NativeHomebrewInstallEvidence,
        String,
        NativeHomebrewHostedProfileEvidence,
    ),
    OrchestratorError,
> {
    let InstallRequest {
        root,
        runner_temp,
        channel,
        identity,
        event,
        deadline,
        ..
    } = request;
    let source = load_install_source(root, channel, identity, event, deadline)?;
    let installed = extract_install_files(&source, identity.platform)?;
    let custody = install_custody(&request, &source.formula_path)?;
    let hosted = crate::homebrew::hosted_profile::prepare_hosted_homebrew(&custody, deadline)?;
    let install_temp = create_install_paths(runner_temp)?;
    let evidence = run_homebrew_install(&source, &installed, &install_temp.paths, deadline)?;
    Ok((
        evidence,
        source.receipt_digest,
        hosted_profile_evidence(hosted),
    ))
}

fn extract_install_files(
    source: &InstallSource,
    platform: velnor_actions_contract::CheckPlatform,
) -> Result<Vec<crate::homebrew::install::HomebrewInstallFile>, OrchestratorError> {
    use crate::internal::internal;
    use velnor_actions_contract::homebrew_spec::HomebrewAuxiliaryTest;

    let expected_files = crate::internal_plan::native_homebrew_checks::expected_install_files(
        source.authority.spec(),
        &source.projection,
        platform,
    )?;
    let expected_by_asset = expected_asset_members(&expected_files);
    let mut extracted = extract_archive_members(&source.release, expected_by_asset)?;
    let mut installed = Vec::new();
    for file in expected_files {
        let bytes = extracted
            .remove(&(file.asset.clone(), file.member.clone()))
            .ok_or_else(|| internal("native_check_archive_member_missing"))?;
        let elf = if file.test == HomebrewAuxiliaryTest::LinuxElf {
            Some(
                crate::homebrew::install::elf_facts_from_bytes(&bytes)
                    .map_err(|_| internal("native_check_elf_invalid"))?,
            )
        } else {
            None
        };
        if elf.map(|facts| (facts.class, facts.endian, facts.machine)) != file.elf {
            return Err(internal("native_check_elf_policy_mismatch"));
        }
        installed.push(crate::homebrew::install::HomebrewInstallFile {
            asset: file.asset,
            member: file.member,
            destination: file.destination,
            sha256: sha256_hex(&bytes),
            mode: file.mode,
            test: file.test,
            elf,
        });
    }
    if !extracted.is_empty() {
        return Err(internal("native_check_archive_inventory_unexpected"));
    }
    Ok(installed)
}

fn expected_asset_members(
    files: &[crate::internal_plan::native_homebrew_checks::NativeExpectedInstallFile],
) -> BTreeMap<String, BTreeSet<String>> {
    let mut expected = BTreeMap::<String, BTreeSet<String>>::new();
    for file in files {
        expected
            .entry(file.asset.clone())
            .or_default()
            .insert(file.member.clone());
    }
    expected
}

fn extract_archive_members(
    release: &crate::package_release_public::VerifiedPublicPackageRelease,
    expected: BTreeMap<String, BTreeSet<String>>,
) -> Result<BTreeMap<(String, String), Vec<u8>>, OrchestratorError> {
    let mut extracted = BTreeMap::new();
    for (asset, members) in expected {
        let archive_bytes = release.asset_bytes(&asset)?;
        let archive_members = crate::package_release_artifacts::extract_tar_gz_members(
            &archive_bytes,
            &members,
            Instant::now(),
        )?;
        for (member, bytes) in archive_members {
            extracted.insert((asset.clone(), member), bytes);
        }
    }
    Ok(extracted)
}

fn install_custody(
    request: &InstallRequest<'_>,
    formula_path: &str,
) -> Result<VerifiedNativeHomebrewInstall, OrchestratorError> {
    use crate::internal::internal;
    let InstallRequest {
        plan,
        entry,
        obligation,
        identity,
        entry_digest,
        task_id,
        check_id,
        ..
    } = *request;

    let custody = VerifiedNativeHomebrewInstall {
        run_key: plan.run_key.clone(),
        head: plan.head.clone(),
        check_id: check_id.to_owned(),
        operation: NativeHomebrewOperation::Install,
        channel: identity
            .formula_channel
            .ok_or_else(|| internal("native_check_install_identity"))?,
        platform: identity.platform,
        task_id: task_id.to_owned(),
        plan_id: plan.plan_id.clone(),
        entry_digest: entry_digest.to_owned(),
        task_digest: obligation.task_digest.clone(),
        input_digest: obligation.input_digest.clone(),
        closure_digest: obligation.closure_digest.clone(),
        tap_repository: identity.tap_repository.clone(),
        tap_repository_id: identity.tap_repository_id,
        tap_base_sha: identity.tap_base_sha.clone(),
        formula_path: formula_path.to_owned(),
        formula_blob_sha: identity
            .formula_blob_sha
            .clone()
            .ok_or_else(|| internal("native_check_formula_blob_identity"))?,
    };
    if entry.stack_id != "native"
        || obligation.task_id != task_id
        || obligation.decision != ObligationDecision::Execute
        || entry.task_digest != obligation.task_digest
        || entry.input_digest != obligation.input_digest
        || identity.check_id != check_id
        || identity.formula_path.as_deref() != Some(formula_path)
        || identity.formula_blob_sha.as_deref() != Some(custody.formula_blob_sha.as_str())
    {
        return Err(internal("native_check_install_custody"));
    }
    Ok(custody)
}

struct InstallTemp {
    _base: tempfile::TempDir,
    paths: crate::homebrew::install::HomebrewInstallPaths,
}

fn create_install_paths(runner_temp: &Path) -> Result<InstallTemp, OrchestratorError> {
    let base = tempfile::Builder::new()
        .prefix("velnor-native-homebrew-")
        .tempdir_in(runner_temp)
        .map_err(|error| {
            OrchestratorError::io(runner_temp.display().to_string(), error.to_string())
        })?;
    let home = base.path().join("home");
    let temp = base.path().join("temp");
    let staging = base.path().join("staging");
    for directory in [&home, &temp, &staging] {
        fs::create_dir(directory).map_err(|error| {
            OrchestratorError::io(directory.display().to_string(), error.to_string())
        })?;
    }
    secure_install_directories(&base, [home.as_path(), temp.as_path(), staging.as_path()])?;
    Ok(InstallTemp {
        _base: base,
        paths: crate::homebrew::install::HomebrewInstallPaths {
            home,
            temp,
            staging,
        },
    })
}

fn secure_install_directories(
    base: &tempfile::TempDir,
    directories: [&Path; 3],
) -> Result<(), OrchestratorError> {
    use std::os::unix::fs::PermissionsExt;

    for directory in [base.path(), directories[0], directories[1], directories[2]] {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).map_err(|error| {
            OrchestratorError::io(directory.display().to_string(), error.to_string())
        })?;
    }
    Ok(())
}

fn run_homebrew_install(
    source: &InstallSource,
    installed: &[crate::homebrew::install::HomebrewInstallFile],
    paths: &crate::homebrew::install::HomebrewInstallPaths,
    deadline: CheckDeadline,
) -> Result<NativeHomebrewInstallEvidence, OrchestratorError> {
    crate::homebrew::install::install_homebrew_candidate(
        &crate::homebrew::install::HomebrewInstallRequest {
            spec: source.authority.spec(),
            projection: &source.projection,
            literals: &source.literals,
            formula_bytes: &source.formula_bytes,
            formula_sha256: &sha256_hex(&source.formula_bytes),
            files: installed,
            paths,
            deadline,
        },
    )
    .map_err(|error| crate::internal::internal(&error.to_string()))
}

fn hosted_profile_evidence(
    hosted: crate::homebrew::hosted_profile::HostedHomebrewProfile,
) -> NativeHomebrewHostedProfileEvidence {
    NativeHomebrewHostedProfileEvidence {
        source_commit: hosted.source_commit,
        source_tree: hosted.source_tree,
        source_describe: hosted.source_describe,
        source_tag_commit: hosted.source_tag_commit,
        portable_ruby_version: hosted.portable_ruby_version,
        portable_ruby_archive_sha256: hosted.portable_ruby_archive_sha256,
        portable_ruby_binary_sha256: hosted.portable_ruby_binary_sha256,
        active_prefix_device: hosted.active_prefix_device,
        active_prefix_inode: hosted.active_prefix_inode,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
