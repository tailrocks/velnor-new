//! Safe reading and generation behavior for the repository-owned PR template.

use std::fs;
use tempfile::TempDir;
use velnor_actions_contract_release::formats::PULL_REQUEST_TEMPLATE_PATH;
use velnor_actions_orchestrator_generation::generate::{
    GenerateOptions, generate, render_staged_tree,
};
use velnor_actions_orchestrator_generation::prepare::{GenerationPreparation, prepare};

use crate::impl_common::{TestResult, config_with_branch, make_repo};

fn prepared_repo() -> Result<(TempDir, GenerationPreparation), Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    Ok((repo, prep))
}

#[test]
fn absent_template_is_omitted_and_present_template_is_exact() -> TestResult {
    let (repo, prep) = prepared_repo()?;
    let absent = render_staged_tree(&prep)?;
    assert_eq!(absent.get(PULL_REQUEST_TEMPLATE_PATH), None);
    fs::create_dir_all(repo.path().join(".github"))?;
    let missing_leaf = render_staged_tree(&prep)?;
    assert_eq!(missing_leaf, absent);

    let template = "# Please describe this change.\n\n- [ ] checked\n";
    fs::write(repo.path().join(PULL_REQUEST_TEMPLATE_PATH), template)?;
    let present = render_staged_tree(&prep)?;
    assert_eq!(present.get(PULL_REQUEST_TEMPLATE_PATH), Some(template));
    Ok(())
}

#[test]
fn generate_replaces_the_tree_while_carrying_the_template() -> TestResult {
    let (repo, prep) = prepared_repo()?;
    let github = repo.path().join(".github");
    fs::create_dir_all(&github)?;
    let sentinel = github.join("old-output.txt");
    fs::write(&sentinel, "removed by replacement")?;
    let template = "## Pull request checklist\n\n- [ ] reviewed\n";
    fs::write(repo.path().join(PULL_REQUEST_TEMPLATE_PATH), template)?;

    let report = generate(&prep, &GenerateOptions::default())?;
    assert!(
        report
            .files_written
            .iter()
            .any(|path| path == PULL_REQUEST_TEMPLATE_PATH)
    );
    assert_eq!(
        fs::read_to_string(repo.path().join(PULL_REQUEST_TEMPLATE_PATH))?,
        template
    );
    assert!(
        !sentinel.exists(),
        "old generated tree content was replaced"
    );
    Ok(())
}

#[test]
fn invalid_template_fails_before_replacing_existing_github_tree() -> TestResult {
    let (repo, prep) = prepared_repo()?;
    let github = repo.path().join(".github");
    fs::create_dir_all(&github)?;
    let sentinel = github.join("keep-me.txt");
    fs::write(&sentinel, "existing user data")?;
    let template = github.join("PULL_REQUEST_TEMPLATE.md");
    fs::write(&template, [0xff, 0xfe])?;

    let error = generate(&prep, &GenerateOptions::default())
        .expect_err("invalid UTF-8 must refuse generation");
    assert!(error.to_string().contains("invalid utf-8"), "{error}");
    assert_eq!(fs::read_to_string(sentinel)?, "existing user data");
    assert_eq!(fs::read(template)?, [0xff, 0xfe]);
    Ok(())
}

#[test]
fn template_parent_must_be_a_directory() -> TestResult {
    let (repo, prep) = prepared_repo()?;
    fs::write(repo.path().join(".github"), "not a directory")?;
    let error = render_staged_tree(&prep).expect_err("non-directory .github must fail closed");
    assert!(error.to_string().contains("not_a_directory"), "{error}");
    Ok(())
}

#[test]
fn template_must_be_a_regular_file_and_within_size_limit() -> TestResult {
    let (repo, prep) = prepared_repo()?;
    let github = repo.path().join(".github");
    fs::create_dir_all(&github)?;
    let template = github.join("PULL_REQUEST_TEMPLATE.md");
    fs::create_dir(&template)?;
    let error = render_staged_tree(&prep).expect_err("directory template must fail closed");
    assert!(error.to_string().contains("not_a_file"), "{error}");
    fs::remove_dir(&template)?;
    fs::write(&template, vec![b'x'; 65_537])?;
    let error = render_staged_tree(&prep).expect_err("oversized template must fail closed");
    assert!(error.to_string().contains("oversize"), "{error}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn template_path_symlinks_are_rejected() -> TestResult {
    use std::os::unix::fs::symlink;

    let (repo, prep) = prepared_repo()?;
    let target = repo.path().join("outside.md");
    fs::write(&target, "outside")?;
    fs::create_dir_all(repo.path().join(".github"))?;
    symlink(&target, repo.path().join(PULL_REQUEST_TEMPLATE_PATH))?;
    let error = render_staged_tree(&prep).expect_err("template symlink must fail closed");
    assert!(error.to_string().contains("symlink_refused"), "{error}");

    fs::remove_file(repo.path().join(PULL_REQUEST_TEMPLATE_PATH))?;
    fs::remove_dir(repo.path().join(".github"))?;
    let target_dir = repo.path().join("outside-github");
    fs::create_dir(&target_dir)?;
    symlink(&target_dir, repo.path().join(".github"))?;
    let error = render_staged_tree(&prep).expect_err(".github symlink must fail closed");
    assert!(error.to_string().contains("symlink_refused"), "{error}");
    Ok(())
}
