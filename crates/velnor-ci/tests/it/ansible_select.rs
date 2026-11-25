// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{
    ChangeSet, EventKind, PackageName, RepoPath, RepoView, RustMetadata, RustPackage, Target,
    TargetKind, TargetName, WORKFLOW_PATH,
};
use velnor_rust::{CONFIG_PATH, ansible_runs, detect, unknown_paths};

type TestResult = Result<(), Box<dyn Error>>;

fn put(view: &mut RepoView, path: &str, bytes: &[u8]) -> TestResult {
    let path = RepoPath::new(path)?;
    view.tracked_paths.insert(path.clone());
    view.files.insert(path, bytes.to_vec());
    Ok(())
}

fn requirements(view: &mut RepoView) -> TestResult {
    let hash = "0123456789abcdef".repeat(4);
    let lint = "fedcba9876543210".repeat(4);
    let text = format!(
        "ansible-core==2.16.14 \\\n    --hash=sha256:{hash}\nansible-lint==24.2.0 \\\n    --hash=sha256:{lint}\n"
    );
    put(view, "ansible-configs/requirements.txt", text.as_bytes())?;
    put(
        view,
        "ansible-configs/requirements.yaml",
        b"collections: []\n",
    )
}

fn play() -> Result<RepoView, Box<dyn Error>> {
    let mut view = RepoView::default();
    requirements(&mut view)?;
    put(
        &mut view,
        "ansible-configs/site.yml",
        b"---\n- hosts: all\n",
    )?;
    Ok(view)
}

fn changed(paths: &[&str]) -> Result<ChangeSet, Box<dyn Error>> {
    let mut set = ChangeSet::default();
    for path in paths {
        set.paths.insert(RepoPath::new(*path)?);
    }
    Ok(set)
}

fn runs(
    view: &RepoView,
    facts: &velnor_model::RepoFacts,
    event: EventKind,
    paths: &[&str],
) -> Result<Option<bool>, Box<dyn Error>> {
    let set = changed(paths)?;
    let unknown = unknown_paths(facts, &set);
    if paths == ["notes/stray.txt"] {
        assert!(
            unknown
                .iter()
                .any(|path| path.as_str() == "notes/stray.txt"),
            "{unknown:?}"
        );
    }
    Ok(ansible_runs(
        view,
        facts,
        event,
        Some(&set.paths),
        &unknown,
    )?)
}

#[test]
fn ansible_runs_for_every_changed_path_when_a_play_exists() -> TestResult {
    let view = play()?;
    let facts = detect(&view)?;
    for path in [
        "ansible-configs/requirements.yaml",
        "ansible-configs/site.yml",
        "ansible-configs/roles/x",
        WORKFLOW_PATH.as_str(),
        CONFIG_PATH.as_str(),
        "notes/stray.txt",
        "README.md",
        "docs/tasks.yml",
    ] {
        assert_eq!(
            runs(&view, &facts, EventKind::PullRequest, &[path])?,
            Some(true),
            "{path}"
        );
    }
    assert_eq!(
        runs(&view, &facts, EventKind::Push, &["README.md"])?,
        Some(true)
    );
    let mut rooted = play()?;
    let manifest = RepoPath::new("Cargo.toml")?;
    rooted.tracked_paths.insert(manifest.clone());
    rooted.tracked_paths.insert(RepoPath::new("Cargo.lock")?);
    put(&mut rooted, "mise.toml", b"[tools]\n")?;
    put(
        &mut rooted,
        "Cargo.toml",
        b"[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )?;
    rooted.rust_metadata.insert(
        manifest.clone(),
        RustMetadata {
            members: vec![RustPackage {
                features: std::collections::BTreeMap::new(),
                name: PackageName::new("app")?,
                version: Some("0.1.0".to_owned()),
                manifest,
                dependencies: Vec::new(),
                targets: vec![Target {
                    name: TargetName::new("app")?,
                    kinds: vec![TargetKind::Lib],
                    tests: false,
                    doctest: false,
                    source: None,
                    required_features: Vec::new(),
                    doc: true,
                    bench: true,
                }],
            }],
        },
    );
    let facts = detect(&rooted)?;
    let set = changed(&["src/deploy.yml"])?;
    let unknown = unknown_paths(&facts, &set);
    assert!(
        !unknown.iter().any(|path| path.as_str() == "src/deploy.yml"),
        "{unknown:?}"
    );
    assert_eq!(
        ansible_runs(
            &rooted,
            &facts,
            EventKind::PullRequest,
            Some(&set.paths),
            &unknown,
        )?,
        Some(true)
    );
    let mut quiet = RepoView::default();
    requirements(&mut quiet)?;
    put(
        &mut quiet,
        "ansible-configs/notes.yml",
        b"---\n- name: notes\n",
    )?;
    let facts = detect(&quiet)?;
    assert_eq!(
        runs(&quiet, &facts, EventKind::PullRequest, &["README.md"])?,
        None
    );
    Ok(())
}
