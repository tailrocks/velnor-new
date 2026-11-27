// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{DependencyKind, EventKind, RepoPath, RepoView, RustPackage, TargetKind};
use velnor_rust::{detect, lint_runs};

use crate::facts::{TestResult, package, path, repo};

pub(crate) const GATED: &str =
    "[[lint]]\nkind = 'generated-check'\nfrom_source = true\ngate_on_plan = true\n";
const GENERATOR: &str = "crates/velnor-gen/Cargo.toml";
const ENGINE: &str = "crates/engine/Cargo.toml";
const OTHER: &str = "crates/other/Cargo.toml";
const NORMAL: DependencyKind = DependencyKind::Normal;

pub(crate) struct World {
    pub(crate) packages: Vec<RustPackage>,
    pub(crate) files: Vec<(String, String)>,
    pub(crate) config: String,
}

impl World {
    pub(crate) fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let generator = package(
            "velnor-gen",
            GENERATOR,
            &[("engine", NORMAL, ENGINE)],
            &[("velnor-gen", TargetKind::Bin, false)],
        )?;
        let engine = package("engine", ENGINE, &[], &[("engine", TargetKind::Lib, true)])?;
        let other = package("other", OTHER, &[], &[("other", TargetKind::Lib, true)])?;
        Ok(Self {
            packages: vec![generator, engine, other],
            files: ["crates/velnor-gen/src/main.rs", "crates/engine/src/lib.rs"]
                .into_iter()
                .chain(["crates/other/src/lib.rs", "shared/borrowed.rs"])
                .chain(["docs/goals.md", "docs/embedded.md", "other/notes.md"])
                .map(|file| (file.to_owned(), String::new()))
                .collect(),
            config: GATED.to_owned(),
        })
    }

    pub(crate) fn file(mut self, name: &str, text: &str) -> Self {
        self.files.retain(|(file, _)| file != name);
        self.files.push((name.to_owned(), text.to_owned()));
        self
    }

    pub(crate) fn view(&self) -> Result<RepoView, Box<dyn std::error::Error>> {
        let mut files: Vec<(&str, &str)> = self
            .files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect();
        files.push((".velnor/rust-ci.toml", &self.config));
        repo(vec![("Cargo.toml", self.packages.clone())], &files)
    }

    pub(crate) fn reaches(
        &self,
        changed: &[&str],
        observed: &[&str],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let view = self.view()?;
        let facts = detect(&view)?;
        let paths = |names: &[&str]| -> Result<BTreeSet<RepoPath>, Box<dyn std::error::Error>> {
            names.iter().map(|name| path(name)).collect()
        };
        lint_runs(
            &facts,
            EventKind::PullRequest,
            Some(&paths(changed)?),
            &view.tracked_paths,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &paths(observed)?,
        )
        .into_iter()
        .find(|(slug, _)| slug.as_str() == "generated_check")
        .map(|(_, run)| run)
        .ok_or_else(|| "no plan decision for the generated check".into())
    }
}

#[test]
fn a_change_outside_the_generator_closure_does_not_reach_the_generated_check() -> TestResult {
    let world = World::new()?;
    assert!(!world.reaches(&["crates/other/src/lib.rs"], &[])?);
    assert!(!world.reaches(&["docs/goals.md", "other/notes.md"], &[])?);
    assert!(world.reaches(&["crates/other/src/lib.rs"], &["crates/other/src/lib.rs"])?);
    Ok(())
}

#[test]
fn a_change_in_a_generator_dependency_reaches_the_generated_check() -> TestResult {
    let world = World::new()?;
    assert!(world.reaches(&["crates/engine/src/lib.rs"], &[])?);
    assert!(world.reaches(&["crates/velnor-gen/src/main.rs"], &[])?);
    assert!(world.reaches(&[".velnor/rust-ci.toml"], &[])?);
    assert!(!world.reaches(&["crates/other/src/lib.rs"], &[])?);
    Ok(())
}

#[test]
fn a_markdown_file_a_package_includes_reaches_the_generated_check() -> TestResult {
    let world = World::new()?.file(
        "crates/other/src/lib.rs",
        "const TEXT: &str = include_str!(\"../../../docs/embedded.md\");\n",
    );
    assert!(world.reaches(&["docs/embedded.md"], &[])?);
    assert!(!world.reaches(&["docs/goals.md"], &[])?);
    Ok(())
}

#[test]
fn a_markdown_file_named_by_a_check_configuration_reaches_the_generated_check() -> TestResult {
    let mut world = World::new()?;
    world.config = format!(
        "{GATED}[[check_configurations]]\nroot = '.'\nconfigurations = 'docs/embedded.md'\n"
    );
    assert!(world.reaches(&["docs/embedded.md"], &[])?);
    assert!(!world.reaches(&["docs/goals.md"], &[])?);
    Ok(())
}

#[test]
fn a_rust_file_a_closure_package_pulls_in_with_a_path_attribute_reaches_the_generated_check()
-> TestResult {
    let world = World::new()?.file(
        "crates/engine/src/lib.rs",
        "#[path = \"../../../shared/borrowed.rs\"]\nmod borrowed;\n",
    );
    assert!(world.reaches(&["shared/borrowed.rs"], &[])?);
    assert!(!world.reaches(&["docs/goals.md"], &[])?);
    Ok(())
}

#[test]
fn a_path_attribute_below_an_inline_module_or_without_a_literal_reaches_every_path() -> TestResult {
    for source in [
        "mod inline {\n    #[path = \"../../../../shared/borrowed.rs\"]\n    mod borrowed;\n}\n",
        "#[cfg_attr(unix, path = concat!(\"../../../\", \"shared/borrowed.rs\"))]\nmod borrowed;\n",
    ] {
        let world = World::new()?.file("crates/engine/src/lib.rs", source);
        assert!(world.reaches(&["docs/goals.md"], &[])?, "{source}");
    }
    Ok(())
}

#[test]
fn an_include_a_closure_package_cannot_resolve_reaches_every_path() -> TestResult {
    for source in [
        "const T: &str = include_str!(concat!(\"../../../docs/\", \"goals.md\"));\n",
        "macro_rules! text { ($p:literal) => { include_str!($p) }; }\nconst T: &str = text!(\"x\");\n",
    ] {
        let world = World::new()?.file("crates/engine/src/lib.rs", source);
        assert!(world.reaches(&["docs/goals.md"], &[])?, "{source}");
        assert!(world.reaches(&["other/notes.md"], &[])?, "{source}");
        let outside = World::new()?.file("crates/other/src/lib.rs", source);
        assert!(!outside.reaches(&["docs/goals.md"], &[])?, "{source}");
    }
    Ok(())
}

#[test]
fn a_target_source_outside_its_package_directory_reaches_the_generated_check() -> TestResult {
    let mut world = World::new()?;
    world.packages[1].targets[0].source = Some(path("shared/borrowed.rs")?);
    assert!(world.reaches(&["shared/extra.rs"], &[])?);
    assert!(world.reaches(&["shared/notes.md"], &[])?);
    assert!(!world.reaches(&["crates/other/src/lib.rs"], &[])?);
    Ok(())
}

#[test]
fn a_build_script_or_a_proc_macro_in_the_closure_reaches_every_path() -> TestResult {
    for kind in [TargetKind::BuildScript, TargetKind::ProcMacro] {
        let mut world = World::new()?;
        world.packages[1].targets[0].kinds = vec![kind];
        assert!(world.reaches(&["docs/goals.md"], &[])?, "{kind:?}");
        assert!(
            world.reaches(&["crates/other/src/lib.rs"], &[])?,
            "{kind:?}"
        );
        let mut outside = World::new()?;
        outside.packages[2].targets[0].kinds = vec![kind];
        assert!(!outside.reaches(&["docs/goals.md"], &[])?, "{kind:?}");
    }
    Ok(())
}

#[test]
fn a_package_the_closure_reaches_only_through_a_patch_reaches_the_generated_check() -> TestResult {
    let mut world = World::new()?;
    world.packages[0].dependencies.clear();
    world.files.push((
        "Cargo.toml".to_owned(),
        "[patch.crates-io]\nengine = { path = \"crates/engine\" }\n".to_owned(),
    ));
    world.packages[0]
        .dependencies
        .push(velnor_model::PackageDependency {
            name: velnor_model::PackageName::new("engine")?,
            kind: NORMAL,
            platform: None,
            optional: false,
            manifest: None,
            features: Vec::new(),
            rename: None,
        });
    assert!(world.reaches(&["crates/engine/src/lib.rs"], &[])?);
    assert!(!world.reaches(&["crates/other/src/lib.rs"], &[])?);
    Ok(())
}
