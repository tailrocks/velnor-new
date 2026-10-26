// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};

use velnor_actions_contract::homebrew_spec::{
    HomebrewFormulaReleaseLiterals, HomebrewFormulaSpec, render_homebrew_formula,
    validate_homebrew_projection,
};
use velnor_actions_mise::CheckDeadline;

use super::sha256;
use crate::{OrchestratorError, internal::internal};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub(super) struct PreparedInstall {
    projection: velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    literals: HomebrewFormulaReleaseLiterals,
    formula: Vec<u8>,
    formula_sha: String,
    files: Vec<crate::homebrew::install::HomebrewInstallFile>,
    paths: crate::homebrew::install::HomebrewInstallPaths,
}

impl PreparedInstall {
    pub(super) fn request<'a>(
        &'a self,
        spec: &'a HomebrewFormulaSpec,
        deadline: CheckDeadline,
    ) -> crate::homebrew::install::HomebrewInstallRequest<'a> {
        crate::homebrew::install::HomebrewInstallRequest {
            spec,
            projection: &self.projection,
            literals: &self.literals,
            formula_bytes: &self.formula,
            formula_sha256: &self.formula_sha,
            files: &self.files,
            paths: &self.paths,
            deadline,
        }
    }
}

pub(super) fn make_request(
    spec: &HomebrewFormulaSpec,
    plan: &velnor_actions_contract::PackageReleasePlan,
    release: &crate::package_release_public::VerifiedPublicPackageRelease,
    record: &super::HistoricalRecord,
    root: &Path,
) -> Result<PreparedInstall, OrchestratorError> {
    use velnor_actions_contract::config::CheckPlatform;
    let projection = validate_homebrew_projection(spec, plan)
        .map_err(|_| internal("homebrew_qualification_projection"))?;
    let hashes = selected_payload_hashes(&projection, &release.receipt().assets)?;
    let literals = HomebrewFormulaReleaseLiterals::new(
        spec,
        &projection,
        record.release.source_sha.clone(),
        record.release.version.clone(),
        record.release.tag.clone(),
        hashes,
    )
    .map_err(|_| internal("homebrew_qualification_literals"))?;
    let formula = render_homebrew_formula(spec, &literals)
        .map_err(|_| internal("homebrew_qualification_render"))?;
    let formula_sha = sha256(&formula);
    let expected = crate::internal_plan::native_homebrew_checks::expected_install_files(
        spec,
        &projection,
        CheckPlatform::MacosArm64,
    )?;
    let mut grouped = BTreeMap::<String, BTreeSet<String>>::new();
    for item in &expected {
        grouped
            .entry(item.asset.clone())
            .or_default()
            .insert(item.member.clone());
    }
    let extracted = extract_expected_members(release, grouped)?;
    let files = bind_install_files(expected, extracted)?;
    let paths = create_install_paths(root)?;
    Ok(PreparedInstall {
        projection,
        literals,
        formula,
        formula_sha,
        files,
        paths,
    })
}

fn selected_payload_hashes(
    projection: &velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    assets: &[velnor_actions_contract::package_release::PackageReleaseAsset],
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut selected = BTreeSet::new();
    let mut hashes = BTreeMap::new();
    for payload in projection.selected_payloads() {
        let name = payload.asset_name();
        if !selected.insert(name) {
            return Err(internal(
                "homebrew_qualification_payload_duplicate_projection",
            ));
        }
        let mut matches = assets.iter().filter(|asset| asset.name == name);
        let asset = matches
            .next()
            .ok_or_else(|| internal("homebrew_qualification_payload_digest_missing"))?;
        if matches.next().is_some() {
            return Err(internal("homebrew_qualification_payload_digest_duplicate"));
        }
        hashes.insert(name.to_owned(), asset.sha256.clone());
    }
    Ok(hashes)
}

fn extract_expected_members(
    release: &crate::package_release_public::VerifiedPublicPackageRelease,
    grouped: BTreeMap<String, BTreeSet<String>>,
) -> Result<BTreeMap<(String, String), Vec<u8>>, OrchestratorError> {
    let mut extracted = BTreeMap::new();
    for (asset, members) in grouped {
        let bytes = release.asset_bytes(&asset)?;
        for (member, contents) in crate::package_release_artifacts::extract_tar_gz_members(
            &bytes,
            &members,
            Instant::now(),
        )? {
            extracted.insert((asset.clone(), member), contents);
        }
    }
    Ok(extracted)
}

fn bind_install_files(
    expected: Vec<crate::internal_plan::native_homebrew_checks::NativeExpectedInstallFile>,
    mut extracted: BTreeMap<(String, String), Vec<u8>>,
) -> Result<Vec<crate::homebrew::install::HomebrewInstallFile>, OrchestratorError> {
    let mut files = Vec::new();
    for item in expected {
        let bytes = extracted
            .remove(&(item.asset.clone(), item.member.clone()))
            .ok_or_else(|| internal("homebrew_qualification_payload_missing"))?;
        let elf = item
            .elf
            .map(|_| crate::homebrew::install::elf_facts_from_bytes(&bytes))
            .transpose()?;
        if elf.map(|facts| (facts.class, facts.endian, facts.machine)) != item.elf {
            return Err(internal("homebrew_qualification_payload_elf"));
        }
        files.push(crate::homebrew::install::HomebrewInstallFile {
            asset: item.asset,
            member: item.member,
            destination: item.destination,
            sha256: sha256(&bytes),
            mode: item.mode,
            test: item.test,
            elf,
        });
    }
    if !extracted.is_empty() {
        return Err(internal("homebrew_qualification_payload_extra"));
    }
    Ok(files)
}

fn create_install_paths(
    root: &Path,
) -> Result<crate::homebrew::install::HomebrewInstallPaths, OrchestratorError> {
    std::fs::create_dir_all(root)
        .map_err(|e| OrchestratorError::io(root.display().to_string(), e.to_string()))?;
    let paths = crate::homebrew::install::HomebrewInstallPaths {
        home: root.join("home"),
        temp: root.join("temp"),
        staging: root.join("stage"),
    };
    for path in [&paths.home, &paths.temp, &paths.staging] {
        std::fs::create_dir(path)
            .map_err(|e| OrchestratorError::io(path.display().to_string(), e.to_string()))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| OrchestratorError::io(path.display().to_string(), e.to_string()))?;
    }
    Ok(paths)
}
