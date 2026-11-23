// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{ChangeSet, EventKind, RepoPath, RepoView, WORKFLOW_PATH};
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
fn ansible_runs_on_a_pull_request_only_for_ansible_ci_and_unknown_paths() -> TestResult {
    let view = play()?;
    let facts = detect(&view)?;
    for path in [
        "ansible-configs/requirements.yaml",
        "ansible-configs/site.yml",
        "ansible-configs/roles/x",
        WORKFLOW_PATH.as_str(),
        CONFIG_PATH.as_str(),
        "notes/stray.txt",
    ] {
        assert_eq!(
            runs(&view, &facts, EventKind::PullRequest, &[path])?,
            Some(true),
            "{path}"
        );
    }
    assert_eq!(
        runs(&view, &facts, EventKind::PullRequest, &["README.md"])?,
        Some(false)
    );
    assert_eq!(
        runs(&view, &facts, EventKind::Push, &["README.md"])?,
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
