// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    JournalFile, JournalFileKind, JournalReceipt, OrchestratorError, PackageEnvelope, invalid,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(super) fn collect_packages(
    receipt: &JournalReceipt,
    admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
    home: &Path,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<BTreeMap<String, PackageEnvelope>, OrchestratorError> {
    let tracked: BTreeSet<_> = receipt
        .source_files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    let cargo_home = home.join("cargo-home");
    crate::crate_release_controller::verify_capacity_rust(
        &intent.rust_version,
        &cargo_home,
        deadline,
    )?;
    let workspace = super::features::workspace_manifest(source, intent, git, &tracked, deadline)?;
    crate::crate_release_controller::verify_capacity_rust(
        &intent.rust_version,
        &cargo_home,
        deadline,
    )?;
    let mut packages = BTreeMap::new();
    for (index, (planned, admitted)) in receipt.packages.iter().zip(&admission.packages).enumerate()
    {
        if planned.name != admitted.source.name
            || planned.version != admitted.source.version
            || planned.manifest != admitted.source.manifest
        {
            return Err(invalid());
        }
        let request = crate::CratePackageSource::new(
            source,
            intent,
            &admitted.source,
            home,
            &receipt.identity.source_sha,
            false,
            git,
        );
        let package = package_envelope(&request, &workspace, &tracked, home, deadline, index == 0)?;
        if packages.insert(planned.name.clone(), package).is_some() {
            return Err(invalid());
        }
    }
    Ok(packages)
}

fn package_envelope(
    request: &crate::CratePackageSource<'_>,
    workspace: &toml::Value,
    tracked: &BTreeSet<&str>,
    home: &Path,
    deadline: velnor_actions_mise::CheckDeadline,
    prime_index: bool,
) -> Result<PackageEnvelope, OrchestratorError> {
    let paths = crate::crate_release_controller::list_package_unprivileged(
        request,
        deadline,
        home,
        prime_index,
    )?;
    let directory = Path::new(&request.package.manifest)
        .parent()
        .ok_or_else(invalid)?;
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        if path.len() > 1024
            || path.starts_with('/')
            || path.contains('\\')
            || path.split('/').any(|part| {
                part.is_empty() || part == "." || part == ".." || part.chars().any(char::is_control)
            })
        {
            return Err(invalid());
        }
        let relative = directory.join(&path);
        let relative = relative.to_str().ok_or_else(invalid)?;
        if !tracked.contains(relative)
            && !matches!(
                path.as_str(),
                "Cargo.toml" | "Cargo.toml.orig" | "Cargo.lock" | ".cargo_vcs_info.json"
            )
        {
            return Err(invalid());
        }
        files.push(JournalFile {
            path,
            sha256: "f".repeat(64),
            kind: JournalFileKind::Regular { executable: false },
        });
    }
    let features = super::features::manifest_features(
        request.source,
        &request.package.manifest,
        workspace,
        tracked,
        deadline,
    )?;
    if !super::super::schema::valid_files(&files)
        || features.len() > 10_000
        || features.iter().any(|(name, members)| {
            !super::super::schema::valid_feature(name)
                || members.len() > 10_000
                || members
                    .iter()
                    .any(|member| !super::super::schema::valid_feature(member))
        })
    {
        return Err(invalid());
    }
    Ok(PackageEnvelope { files, features })
}
