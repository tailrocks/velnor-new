// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_with_notes;
use velnor_model::{CACHE_TRIM_WORKFLOW_PATH, RepoPath, RepoView, WORKFLOW_PATH};

type TestResult = Result<(), Box<dyn Error>>;

fn repository_without_cargo_toml() -> Result<velnor_model::RepoView, Box<dyn Error>> {
    let view = crate::pipeline::view(&[], None)?;
    assert!(
        view.tracked_paths
            .iter()
            .chain(view.files.keys())
            .all(|path| !path.as_str().ends_with("Cargo.toml")),
        "the fixture contains a Cargo.toml"
    );
    Ok(view)
}

fn workflows(view: &velnor_model::RepoView) -> Result<Vec<(RepoPath, String)>, Box<dyn Error>> {
    let files = expected_with_notes(view)?.0;
    Ok(files
        .into_iter()
        .filter(|(path, _)| path.as_str().starts_with(".github/workflows/"))
        .collect())
}

fn mentions_rust_tooling(text: &str) -> bool {
    text.lines().any(|line| line == "  rust:")
        || text.contains("cargo")
        || text.contains("mbx")
        || text.contains("rustup")
        || text.contains("nextest")
        || text.contains("rust-toolchain")
}

#[test]
fn generate_accepts_a_repository_with_no_cargo_toml() -> TestResult {
    let view = repository_without_cargo_toml()?;
    expected_with_notes(&view).map(|_| ()).map_err(Into::into)
}

#[test]
fn a_repository_with_no_cargo_toml_receives_no_rust_job() -> TestResult {
    let view = repository_without_cargo_toml()?;
    for (path, text) in workflows(&view)? {
        if mentions_rust_tooling(&text) {
            return Err(format!("{path} has a rust job or Rust tooling:\n{text}").into());
        }
    }
    Ok(())
}

fn put(view: &mut RepoView, path: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let path = RepoPath::new(path)?;
    view.tracked_paths.insert(path.clone());
    view.files.insert(path, bytes.to_vec());
    Ok(())
}

fn ansible_repository(playbook: &str, playbook_path: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut view = RepoView::default();
    let core = "0123456789abcdef".repeat(4);
    let lint = "fedcba9876543210".repeat(4);
    let requirements = format!(
        "ansible-core==2.16.14 \\\n    --hash=sha256:{core}\nansible-lint==24.2.0 \\\n    --hash=sha256:{lint}\n"
    );
    put(
        &mut view,
        "ansible-configs/requirements.txt",
        requirements.as_bytes(),
    )?;
    put(
        &mut view,
        "ansible-configs/requirements.yaml",
        b"collections: []\n",
    )?;
    put(&mut view, playbook_path, playbook.as_bytes())?;
    assert!(
        view.tracked_paths
            .iter()
            .chain(view.files.keys())
            .all(|path| !path.as_str().ends_with("Cargo.toml")),
        "the ansible fixture contains a Cargo.toml"
    );
    Ok(view)
}

#[test]
fn an_ansible_repository_without_cargo_writes_one_ci_workflow_and_no_rust_job() -> TestResult {
    let files = expected_with_notes(&ansible_repository(
        "---\n- hosts: all\n  tasks: []\n",
        "ansible-configs/site.yml",
    )?)?
    .0;
    let workflow = files
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?;
    assert!(
        workflow.lines().any(|line| line == "name: CI"),
        "{workflow}"
    );
    assert!(
        workflow.lines().any(|line| line == "  ansible:"),
        "{workflow}"
    );
    assert!(workflow.contains("--syntax-check"), "{workflow}");
    assert!(workflow.contains("ansiblelint"), "{workflow}");
    assert!(workflow.contains("--require-hashes"), "{workflow}");
    assert!(
        workflow.contains("ansible-configs/requirements.txt"),
        "{workflow}"
    );
    assert!(workflow.contains("ansible-configs/site.yml"), "{workflow}");
    assert!(!mentions_rust_tooling(workflow), "{workflow}");
    let workflows: Vec<_> = files
        .keys()
        .filter(|path| path.as_str().starts_with(".github/workflows/"))
        .map(RepoPath::as_str)
        .collect();
    assert_eq!(workflows, vec![WORKFLOW_PATH.as_str()]);
    Ok(())
}

#[test]
fn a_play_whose_hosts_key_follows_its_name_gets_an_ansible_job() -> TestResult {
    let files = expected_with_notes(&ansible_repository(
        "---\n- name: Configure infrastructure\n  hosts: all\n  tasks: []\n",
        "ansible-configs/site.yml",
    )?)?
    .0;
    let workflow = files
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?;
    assert!(
        workflow.lines().any(|line| line == "name: CI"),
        "{workflow}"
    );
    assert!(
        workflow.lines().any(|line| line == "  ansible:"),
        "{workflow}"
    );
    assert!(
        workflow.contains("--syntax-check ansible-configs/site.yml"),
        "{workflow}"
    );
    assert!(workflow.contains("ansiblelint"), "{workflow}");
    assert!(workflow.contains("--require-hashes"), "{workflow}");
    assert!(!mentions_rust_tooling(workflow), "{workflow}");
    Ok(())
}

#[test]
fn a_yaml_list_without_hosts_or_import_playbook_gets_no_ansible_job() -> TestResult {
    let files = expected_with_notes(&ansible_repository(
        "---\n- name: notes\n  tasks: []\n",
        "ansible-configs/notes.yml",
    )?)?
    .0;
    let workflow = files
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?;
    assert!(
        !workflow.lines().any(|line| line == "  ansible:"),
        "{workflow}"
    );
    assert!(!mentions_rust_tooling(workflow), "{workflow}");
    Ok(())
}

#[test]
fn a_cargo_workspace_writes_ci_with_fmt_clippy_test_mbx_and_cache() -> TestResult {
    let view = crate::pipeline::with_deltas(crate::pipeline::view(&["Cargo.toml"], None)?)?;
    let files = expected_with_notes(&view)?.0;
    let workflow = files
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?;
    assert!(
        workflow.lines().any(|line| line == "name: CI"),
        "{workflow}"
    );
    assert!(workflow.contains("cargo-fmt"), "{workflow}");
    assert!(workflow.contains("cargo-clippy"), "{workflow}");
    assert!(workflow.contains("cargo-nextest"), "{workflow}");
    assert!(workflow.contains("mbx"), "{workflow}");
    assert!(workflow.contains("actions/cache/restore"), "{workflow}");
    assert!(workflow.contains("actions/cache/save"), "{workflow}");
    let trim = files
        .get(&CACHE_TRIM_WORKFLOW_PATH)
        .ok_or("delta cache did not write rust-cache-trim.yml")?;
    assert!(trim.contains(WORKFLOW_PATH.as_str()), "{trim}");
    assert!(!trim.contains("rust-ci.yml"), "{trim}");
    assert!(
        trim.contains("\n  - CI\n") || trim.contains("- CI"),
        "{trim}"
    );
    Ok(())
}
