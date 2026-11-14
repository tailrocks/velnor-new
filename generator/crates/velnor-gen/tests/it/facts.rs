// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{
    DependencyKind, PackageDependency, PackageName, RepoFacts, RepoPath, RepoView, RustMetadata,
    RustPackage, Target, TargetKind, TargetName,
};

pub(crate) type TestResult = Result<(), Box<dyn Error>>;

pub(crate) const MISE: &str = "[tools]\nrust = \"1.99.1\"\n";

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
        manifest: path(manifest)?,
        dependencies: dependencies
            .iter()
            .map(|(name, kind, manifest)| {
                Ok(PackageDependency {
                    name: PackageName::new(*name)?,
                    kind: *kind,
                    manifest: Some(path(manifest)?),
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
                    source: None,
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
        view.rust_metadata
            .insert(manifest, RustMetadata { members });
    }
    for (name, text) in [("mise.toml", MISE)].iter().chain(files) {
        view.tracked_paths.insert(path(name)?);
        view.files.insert(path(name)?, text.as_bytes().to_vec());
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
