//! Commit-boundary drift checks for the repository-owned PR template.

use std::fs;

use tempfile::tempdir;
use velnor_actions_contract_release::formats::PULL_REQUEST_TEMPLATE_PATH;
use velnor_actions_workflow_tree::rendered::{RenderedFile, RenderedTree};

use super::super::{preserved_template, replace_in_place};

fn rendered_tree(template: Option<&str>) -> RenderedTree {
    let mut files = vec![RenderedFile {
        path: ".github/workflows/ci.yml".to_owned(),
        bytes: "name: CI\n".to_owned(),
    }];
    if let Some(template) = template {
        files.push(RenderedFile {
            path: PULL_REQUEST_TEMPLATE_PATH.to_owned(),
            bytes: template.to_owned(),
        });
    }
    RenderedTree {
        files,
        symlinks: Vec::new(),
    }
}

fn assert_drift_refused(
    before: Option<&str>,
    after: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = tempdir()?;
    let github = repo.path().join(".github");
    fs::create_dir(&github)?;
    let sentinel = github.join("old-user-file.txt");
    fs::write(&sentinel, "keep the old tree")?;
    let template = repo.path().join(PULL_REQUEST_TEMPLATE_PATH);
    if let Some(contents) = before {
        fs::write(&template, contents)?;
    }

    let snapshot = preserved_template::read(repo.path())?;
    assert_eq!(snapshot.as_deref(), before);
    let tree = rendered_tree(snapshot.as_deref());
    match after {
        Some(contents) => fs::write(&template, contents)?,
        None => fs::remove_file(&template)?,
    }

    let Err(error) = replace_in_place(repo.path(), &tree, snapshot.as_deref()) else {
        return Err("template drift after render must refuse replacement".into());
    };
    assert!(
        error
            .to_string()
            .contains("preserved_template_changed_during_generation"),
        "{error}"
    );
    assert_eq!(fs::read_to_string(&sentinel)?, "keep the old tree");
    match after {
        Some(contents) => assert_eq!(fs::read_to_string(template)?, contents),
        None => assert!(!template.exists(), "deleted input stays deleted"),
    }
    Ok(())
}

#[test]
fn edited_template_after_render_refuses_replacement() -> Result<(), Box<dyn std::error::Error>> {
    assert_drift_refused(Some("before\n"), Some("edited after render\n"))
}

#[test]
fn created_template_after_render_refuses_replacement() -> Result<(), Box<dyn std::error::Error>> {
    assert_drift_refused(None, Some("created after render\n"))
}

#[test]
fn deleted_template_after_render_refuses_replacement() -> Result<(), Box<dyn std::error::Error>> {
    assert_drift_refused(Some("before\n"), None)
}
