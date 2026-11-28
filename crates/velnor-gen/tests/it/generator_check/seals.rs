// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoView, Target, TargetKind, TargetName};
use velnor_rust::detect;

use super::world::{
    CONFIG, CORE, ENGINE, GEN, OTHER, PINNED, QUIET, decide, decide_in_place, manifest, put,
    runs_every, set_config, skips_every, world,
};
use crate::facts::{TestResult, path};

fn with_target(view: &mut RepoView, root: &str, member: usize, kind: TargetKind) -> TestResult {
    let metadata = view.rust_metadata.get_mut(&path(root)?).ok_or("no root")?;
    metadata.members[member].targets.push(Target {
        name: TargetName::new("extra")?,
        kinds: vec![kind],
        tests: false,
        doctest: false,
        source: None,
        requires_features: false,
        required_features: Vec::new(),
    });
    Ok(())
}

fn forget(view: &mut RepoView, file: &str) -> TestResult {
    view.tracked_paths.remove(&path(file)?);
    view.files.remove(&path(file)?);
    Ok(())
}

fn stray_dependency(view: &mut RepoView) -> TestResult {
    let root = path("Cargo.toml")?;
    let stray = path("crates/stray/Cargo.toml")?;
    let metadata = view.rust_metadata.get_mut(&root).ok_or("no root")?;
    metadata.members[0]
        .dependencies
        .push(velnor_model::PackageDependency {
            name: velnor_model::PackageName::new("stray")?,
            kind: velnor_model::DependencyKind::Normal,
            platform: None,
            optional: false,
            manifest: Some(stray),
            features: Vec::new(),
            rename: None,
        });
    put(view, "crates/stray/Cargo.toml", &manifest("stray"))
}

#[test]
fn each_seal_makes_every_path_reach_the_generated_check() -> TestResult {
    let control = world()?;
    assert!(skips_every(&control, &QUIET)?, "the unsealed world skips");
    let mut cases: Vec<(&str, RepoView)> = Vec::new();
    let mut missing = world()?;
    forget(&mut missing, ".velnor/rust-inputs.lock")?;
    cases.push(("lock missing", missing));
    let mut unidentified = world()?;
    put(
        &mut unidentified,
        ".velnor/rust-inputs.lock",
        "shared/borrowed.rs\tcore\n",
    )?;
    cases.push(("lock without manifests", unidentified));
    let mut empty = world()?;
    put(&mut empty, ".velnor/rust-inputs.lock", "")?;
    cases.push(("empty lock", empty));
    let mut script = world()?;
    with_target(&mut script, "Cargo.toml", 2, TargetKind::BuildScript)?;
    cases.push(("closure build script", script));
    let mut macros = world()?;
    with_target(&mut macros, "Cargo.toml", 1, TargetKind::ProcMacro)?;
    cases.push(("closure proc-macro", macros));
    let mut stray = world()?;
    stray_dependency(&mut stray)?;
    cases.push(("closure package outside every workspace", stray));
    let mut skipped = Vec::new();
    for (name, view) in &cases {
        if !runs_every(view, &QUIET)? {
            skipped.push(*name);
        }
    }
    assert!(
        skipped.is_empty(),
        "seals that let a path skip: {skipped:?}"
    );
    Ok(())
}

#[test]
fn a_helper_older_than_the_pull_request_audit_seals_the_check() -> TestResult {
    for (version, sealed) in [
        ("0.1.115", true),
        ("0.1.116", true),
        (PINNED, false),
        ("0.1.118", false),
    ] {
        let mut view = world()?;
        crate::pipeline::pin_helper(&mut view, version)?;
        assert_eq!(runs_every(&view, &QUIET)?, sealed, "{version}");
        assert_eq!(skips_every(&view, &QUIET)?, !sealed, "{version}");
    }
    Ok(())
}

#[test]
fn a_closure_without_a_generator_package_seals_the_check() -> TestResult {
    let view = world()?;
    let mut facts = detect(&view)?;
    let unsealed = decide(&facts, &view.tracked_paths, &["docs/goals.md"], &[], &[])?;
    facts.rust_roots.clear();
    let sealed = decide(&facts, &view.tracked_paths, &["docs/goals.md"], &[], &[])?;
    assert!(!unsealed && sealed);
    Ok(())
}

#[test]
fn a_cargo_config_that_redirects_a_source_seals_the_check() -> TestResult {
    let redirects = [
        "[source.crates-io]\nreplace-with = \"vendored\"\n\n[source.vendored]\ndirectory = \"vendor\"\n",
        "[source.crates-io]\nreplace-with = \"mirror\"\n\n[source.mirror]\nlocal-registry = \"registry\"\n",
        "[source.crates-io]\nreplace-with = \"fork\"\n\n[source.fork]\ngit = \"https://example.invalid/fork\"\n",
        "paths = [\"../override\"]\n",
    ];
    for config in redirects {
        for file in [".cargo/config.toml", ".cargo/config"] {
            let mut view = world()?;
            put(&mut view, file, config)?;
            assert!(runs_every(&view, &QUIET)?, "{file}: {config}");
        }
    }
    let mut broken = world()?;
    put(&mut broken, ".cargo/config.toml", "[source.crates-io\n")?;
    assert!(detect(&broken).is_err());
    let mut benign = world()?;
    put(
        &mut benign,
        ".cargo/config.toml",
        "[alias]\nb = \"build\"\n",
    )?;
    assert!(skips_every(&benign, &QUIET)?);
    let mut elsewhere = world()?;
    put(
        &mut elsewhere,
        "tools/.cargo/config.toml",
        "paths = [\"x\"]\n",
    )?;
    assert!(skips_every(&elsewhere, &QUIET)?);
    Ok(())
}

#[test]
fn a_locked_package_from_outside_a_registry_seals_the_check() -> TestResult {
    let git = "version = 4\n\n[[package]]\nname = \"fork\"\nversion = \"0.1.0\"\nsource = \"git+https://example.invalid/fork#abc\"\n";
    let sparse = |index: &str| {
        format!(
            "version = 4\n\n[[package]]\nname = \"fnv\"\nversion = \"1.0.7\"\nsource = \"sparse+{index}\"\n"
        )
    };
    let mut view = world()?;
    put(&mut view, "Cargo.lock", git)?;
    assert!(runs_every(&view, &QUIET)?);
    put(
        &mut view,
        "Cargo.lock",
        &sparse("https://index.example.invalid/"),
    )?;
    assert!(runs_every(&view, &QUIET)?);
    put(&mut view, "Cargo.lock", &sparse("https://index.crates.io/"))?;
    assert!(skips_every(&view, &QUIET)?);
    put(&mut view, "Cargo.lock", "version = 4\n[[package\n")?;
    assert!(runs_every(&view, &QUIET)?);
    Ok(())
}

#[test]
fn a_closure_package_whose_features_or_platform_dependencies_pick_a_build_seals_the_check()
-> TestResult {
    let picks = [
        "[features]\ndefault = []\nextra = []\n",
        "[target.'cfg(unix)'.dependencies]\nfnv = \"1\"\n",
        "[target.x86_64-pc-windows-msvc.build-dependencies]\nfnv = \"1\"\n",
    ];
    for member in [GEN, ENGINE, CORE] {
        for extra in picks {
            let mut view = world()?;
            put(&mut view, member, &format!("{}{extra}", manifest("member")))?;
            assert!(runs_every(&view, &QUIET)?, "{member}: {extra}");
        }
    }
    let mut outside = world()?;
    put(
        &mut outside,
        OTHER,
        &format!("{}{}", manifest("other"), picks[0]),
    )?;
    assert!(skips_every(&outside, &QUIET)?);
    let mut unread = world()?;
    unread.files.remove(&path(CORE)?);
    assert!(runs_every(&unread, &QUIET)?);
    let mut plain = world()?;
    put(
        &mut plain,
        CORE,
        &format!("{}\n[features]\n", manifest("core")),
    )?;
    assert!(skips_every(&plain, &QUIET)?);
    Ok(())
}

#[test]
fn an_edited_or_retargeted_symlink_runs_and_an_untouched_one_outside_the_generator_does_not()
-> TestResult {
    let view = world()?;
    let facts = detect(&view)?;
    let tracked = &view.tracked_paths;
    let link = "docs/link.txt";
    let goals = "docs/goals.md";
    assert!(!decide(&facts, tracked, &[goals], &[], &[link])?);
    assert!(decide(&facts, tracked, &[link], &[link], &[link])?);
    assert!(decide(&facts, tracked, &[link, goals], &[link], &[link])?);
    let inside = "crates/core/src/data.md";
    assert!(decide(&facts, tracked, &[goals], &[], &[inside])?);
    assert!(decide(
        &facts,
        tracked,
        &[goals],
        &[],
        &["shared/borrowed.rs"]
    )?);
    Ok(())
}

#[test]
fn an_in_place_edit_of_a_lint_task_file_runs_the_generated_check() -> TestResult {
    let mut view = world()?;
    let config = format!(
        "{CONFIG}\n[[lint]]\nkind = 'mise-task'\nid = 'notes'\ntask = 'lint-notes'\ninputs = ['notes/**']\n"
    );
    set_config(&mut view, &config)?;
    put(&mut view, "mise-tasks/lint-notes.md", "task\n")?;
    let facts = detect(&view)?;
    let tracked = &view.tracked_paths;
    assert!(decide_in_place(
        &facts,
        tracked,
        "mise-tasks/lint-notes.md"
    )?);
    assert!(decide_in_place(
        &facts,
        tracked,
        "mise-tasks/lint-notes.rs"
    )?);
    assert!(!decide_in_place(&facts, tracked, "mise-tasks/other.md")?);
    Ok(())
}
