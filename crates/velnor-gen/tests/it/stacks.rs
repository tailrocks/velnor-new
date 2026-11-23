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

pub(crate) fn put(view: &mut RepoView, path: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let path = RepoPath::new(path)?;
    view.tracked_paths.insert(path.clone());
    view.files.insert(path, bytes.to_vec());
    Ok(())
}

const PREFIX: &str = "PYTHONPATH=\"$RUNNER_TEMP/ansible\" PATH=\"$PATH:$RUNNER_TEMP/ansible/bin\"";

pub(crate) fn ansible_repository(
    playbook: &str,
    playbook_path: &str,
) -> Result<RepoView, Box<dyn Error>> {
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

fn runs(workflow: &str, id: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let tree = crate::yaml_tree::document(workflow)?;
    let job = tree
        .get("jobs")
        .and_then(|jobs| jobs.get(id))
        .ok_or_else(|| format!("no {id} job"))?;
    Ok(crate::lint_lane::runs(job))
}

fn command<'a>(runs: &'a [String], marker: &str) -> Result<&'a str, Box<dyn Error>> {
    runs.iter()
        .find(|run| run.contains(marker))
        .map(String::as_str)
        .ok_or_else(|| format!("no {marker} step").into())
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
    assert!(!workflow.contains("GITHUB_ENV"), "{workflow}");
    assert!(!workflow.contains("actions/cache"), "{workflow}");
    assert!(!workflow.contains("paths:"), "{workflow}");
    assert!(!workflow.contains("rust-plan"), "{workflow}");
    assert!(!workflow.contains("velnor-ci gate"), "{workflow}");
    assert!(!workflow.contains("hashFiles"), "{workflow}");
    let steps = runs(workflow, "ansible")?;
    let pip = command(&steps, "pip install")?;
    assert!(!pip.contains("PATH="), "{pip}");
    let galaxy = command(&steps, "ansible.cli.galaxy")?;
    assert_eq!(
        galaxy,
        format!(
            "{PREFIX} python3 -m ansible.cli.galaxy collection install -r ansible-configs/requirements.yaml"
        )
    );
    let syntax = command(&steps, "--syntax-check")?;
    assert!(syntax.starts_with(PREFIX), "{syntax}");
    assert!(syntax.contains("'ansible-configs/site.yml'"), "{syntax}");
    let lint = command(&steps, "ansiblelint")?;
    assert!(lint.starts_with(PREFIX), "{lint}");
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
    let steps = runs(workflow, "ansible")?;
    let syntax = command(&steps, "--syntax-check")?;
    assert!(
        syntax.contains("--syntax-check 'ansible-configs/site.yml'"),
        "{syntax}"
    );
    assert!(
        !syntax.contains("--syntax-check ansible-configs/site.yml"),
        "{syntax}"
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
    assert!(
        !files.contains_key(&WORKFLOW_PATH),
        "a list that is not a play still wrote ci.yml"
    );
    Ok(())
}

#[test]
fn a_dash_on_its_own_line_uses_the_following_key_indent() -> TestResult {
    let path = "ansible-configs/site.yml";
    let files = expected_with_notes(&ansible_repository(
        "-\n    hosts: all\n    tasks: []\n",
        path,
    )?)?
    .0;
    let workflow = files
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?;
    assert!(
        workflow.lines().any(|line| line == "  ansible:"),
        "{workflow}"
    );
    let steps = runs(workflow, "ansible")?;
    let syntax = command(&steps, "--syntax-check")?;
    assert!(
        syntax.contains(&format!("--syntax-check '{path}'")),
        "{syntax}"
    );
    let nested = expected_with_notes(&ansible_repository(
        "---\n- name: notes\n  vars:\n    hosts: all\n  tasks: []\n",
        "ansible-configs/notes.yml",
    )?)?
    .0;
    assert!(
        !nested.contains_key(&WORKFLOW_PATH),
        "hosts nested under vars still wrote ci.yml"
    );
    Ok(())
}

#[test]
fn playbook_paths_are_bash_single_quoted() -> TestResult {
    let mut view = ansible_repository("---\n- hosts: all\n", "ansible-configs/my site.yml")?;
    put(
        &mut view,
        "ansible-configs/o'reilly.yml",
        b"---\n- hosts: all\n",
    )?;
    let workflow = expected_with_notes(&view)?
        .0
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?
        .clone();
    let syntax = runs(&workflow, "ansible")?
        .into_iter()
        .filter(|run| run.contains("--syntax-check"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(syntax.contains("'ansible-configs/my site.yml'"), "{syntax}");
    assert!(
        syntax.contains("'ansible-configs/o'\\''reilly.yml'"),
        "{syntax}"
    );
    assert!(!syntax.contains("o'reilly.yml"), "{syntax}");
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
