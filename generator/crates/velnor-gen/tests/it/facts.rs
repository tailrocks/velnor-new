// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt::Write;

use velnor_model::{
    DependencyKind, PackageDependency, PackageName, RepoFacts, RepoPath, RepoView, RustMetadata,
    RustPackage, Target, TargetKind, TargetName,
};

pub(crate) type TestResult = Result<(), Box<dyn Error>>;

pub(crate) const MISE: &str = "[tools]\nrust = \"1.99.1\"\n";

pub(crate) fn lock_pinned_tools(view: &mut RepoView) -> Result<(), Box<dyn Error>> {
    let mise_lock = path("mise.lock")?;
    if view
        .files
        .get(&mise_lock)
        .is_some_and(|bytes| !String::from_utf8_lossy(bytes).contains("example.invalid"))
    {
        return Ok(());
    }
    let mut names = std::collections::BTreeSet::new();
    for config in ["mise.toml", ".mise.toml"] {
        let Some(bytes) = view.files.get(&path(config)?) else {
            continue;
        };
        let text = String::from_utf8_lossy(bytes);
        let tools = text
            .lines()
            .skip_while(|line| line.trim() != "[tools]")
            .skip(1)
            .take_while(|line| !line.starts_with('['));
        names.extend(
            tools
                .filter_map(|line| line.split_once(" = "))
                .map(|(name, _)| name.trim().trim_matches('"').to_owned()),
        );
    }
    let mut lock = String::new();
    for name in &names {
        for platform in ["linux-x64", "macos-arm64"] {
            write!(
                lock,
                "[[tools.\"{name}\"]]\nversion = \"1\"\n\n[tools.\"{name}\".\"platforms.{platform}\"]\nurl = \"https://example.invalid/{name}\"\n\n"
            )?;
        }
    }
    if lock.is_empty() {
        return Ok(());
    }
    view.tracked_paths.insert(mise_lock.clone());
    view.files.insert(mise_lock, lock.into_bytes());
    Ok(())
}

pub(crate) fn path(text: &str) -> Result<RepoPath, Box<dyn Error>> {
    Ok(RepoPath::new(text)?)
}

pub(crate) fn package(
    name: &str,
    manifest: &str,
    dependencies: &[(&str, DependencyKind, &str)],
    targets: &[(&str, TargetKind, bool)],
) -> Result<RustPackage, Box<dyn Error>> {
    Ok(RustPackage {
        name: PackageName::new(name)?,
        version: "0.1.0".to_owned(),
        manifest: path(manifest)?,
        dependencies: dependencies
            .iter()
            .map(|(name, kind, manifest)| {
                Ok(PackageDependency {
                    name: PackageName::new(*name)?,
                    kind: *kind,
                    platform: None,
                    manifest: Some(path(manifest)?),
                    features: Vec::new(),
                    rename: None,
                })
            })
            .collect::<Result<_, Box<dyn Error>>>()?,
        targets: targets
            .iter()
            .map(|(name, kind, tests)| {
                Ok(Target {
                    name: TargetName::new(*name)?,
                    kinds: vec![*kind],
                    tests: *tests,
                    doctest: true,
                    source: None,
                    requires_features: false,
                })
            })
            .collect::<Result<_, Box<dyn Error>>>()?,
    })
}

pub(crate) fn lib(name: &str, manifest: &str) -> Result<RustPackage, Box<dyn Error>> {
    package(name, manifest, &[], &[(name, TargetKind::Lib, true)])
}

pub(crate) fn repo(
    workspaces: Vec<(&str, Vec<RustPackage>)>,
    files: &[(&str, &str)],
) -> Result<RepoView, Box<dyn Error>> {
    let mut view = RepoView::default();
    for (manifest, members) in workspaces {
        let manifest = path(manifest)?;
        view.tracked_paths
            .insert(manifest.sibling(&path("Cargo.lock")?));
        view.tracked_paths.insert(manifest.clone());
        for member in &members {
            view.tracked_paths.insert(member.manifest.clone());
        }
        view.rust_metadata
            .insert(manifest, RustMetadata { members });
    }
    for (name, text) in [("mise.toml", MISE)].iter().chain(files) {
        view.tracked_paths.insert(path(name)?);
        view.files.insert(path(name)?, text.as_bytes().to_vec());
    }
    lock_pinned_tools(&mut view)?;
    let lock = path(".velnor/rust-ci.lock")?;
    if !view.files.contains_key(&lock) {
        view.tracked_paths.insert(lock.clone());
        view.files
            .insert(lock, crate::pipeline::HELPER_LOCK.to_vec());
    }
    Ok(view)
}

pub(crate) type Shape = Vec<(String, Vec<(String, Option<String>)>)>;

pub(crate) fn shape(facts: &RepoFacts) -> Shape {
    facts
        .rust_roots
        .iter()
        .map(|root| {
            let members = root
                .members
                .iter()
                .map(|member| {
                    (
                        member.name.to_string(),
                        member.directory.as_ref().map(ToString::to_string),
                    )
                })
                .collect();
            (root.manifest.to_string(), members)
        })
        .collect()
}

pub(crate) fn root(
    manifest: &str,
    members: &[(&str, Option<&str>)],
) -> (String, Vec<(String, Option<String>)>) {
    (
        manifest.to_owned(),
        members
            .iter()
            .map(|(name, directory)| ((*name).to_owned(), directory.map(str::to_owned)))
            .collect(),
    )
}
