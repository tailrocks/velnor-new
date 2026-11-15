// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::{GenerateError, expected_files};
use velnor_model::{PackageName, RepoPath};
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, Root, app, jobs, release_view, steps};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn release_pr_stays_on_a_branch_and_rechecks_the_live_default_branch_tip() -> TestResult {
    let (_, tree) = app()?;
    let all = steps(tree.at(&["jobs", "release-pr"]).ok_or("no release-pr")?);
    let name = |step: &Node| step.get("id").and_then(Node::text).map(str::to_owned);
    let tip = all
        .iter()
        .position(|step| name(step).as_deref() == Some("newest"))
        .ok_or("no tip check")?;
    let run = all[tip].get("run").and_then(Node::text).ok_or("no run")?;
    assert!(
        run.contains("commits/main") && run.contains("env.COMMIT"),
        "{run}"
    );
    let switch = all
        .iter()
        .position(|step| {
            step.get("run").and_then(Node::text) == Some("git switch --force-create main")
        })
        .ok_or("no branch switch")?;
    assert!(tip < switch);
    for step in &all[tip + 1..] {
        assert_eq!(
            step.get("if").and_then(Node::text),
            Some("${{ steps.newest.outputs.newest == 'true' }}")
        );
    }
    Ok(())
}

#[test]
fn privileged_release_jobs_run_on_a_github_hosted_runner_whatever_the_ci_runner_is() -> TestResult {
    let mut view = release_view(&[APP], Some("[[binaries]]\npackage = \"app\"\n"))?;
    view.files.insert(
        RepoPath::new(".velnor/config.toml")?,
        b"runner = \"self-hosted\"\n".to_vec(),
    );
    let files = expected_files(&view)?;
    let text = files
        .get(&velnor_model::RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    assert!(!text.contains("self-hosted"), "{text}");
    for (id, job) in jobs(&crate::yaml_tree::parse(text)?)? {
        assert_eq!(
            job.get("runs-on").and_then(Node::text),
            Some("ubuntu-latest"),
            "{id}"
        );
    }
    Ok(())
}

#[test]
fn a_release_root_that_is_not_a_detected_cargo_root_is_refused() -> TestResult {
    let typo = Root { dir: "typo", ..APP };
    let mut view = release_view(&[APP], Some("roots = [\"typo\"]\n"))?;
    view.files.insert(
        RepoPath::new("typo/release-plz.toml")?,
        crate::release_fixture::release_plz(&typo).into_bytes(),
    );
    assert_eq!(
        expected_files(&view).err(),
        Some(GenerateError::ReleaseRootWithoutCargoToml(
            "typo".to_owned()
        ))
    );
    Ok(())
}

#[test]
fn release_plz_pinned_only_in_the_dot_mise_file_is_accepted_and_checked() -> TestResult {
    let mut view = release_view(&[APP], Some("[[binaries]]\npackage = \"app\"\n"))?;
    let both = view
        .files
        .remove(&RepoPath::new("mise.toml")?)
        .ok_or("no mise.toml")?;
    let text = String::from_utf8(both)?;
    let (without, release_plz) = text
        .split_once("\"aqua:release-plz")
        .ok_or("no release-plz line")?;
    view.files
        .insert(RepoPath::new("mise.toml")?, without.as_bytes().to_vec());
    view.files.insert(
        RepoPath::new(".mise.toml")?,
        format!("[tools]\n\"aqua:release-plz{release_plz}").into_bytes(),
    );
    view.tracked_paths.insert(RepoPath::new(".mise.toml")?);
    assert!(expected_files(&view).is_ok());
    view.files.insert(
        RepoPath::new(".mise.toml")?,
        b"[tools]\n\"aqua:release-plz/release-plz\" = \"0.4.2\"\n".to_vec(),
    );
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Release(
            ReleaseError::ReleasePlzLockMismatch { .. }
        ))
    ));
    Ok(())
}

#[test]
fn a_mise_file_that_runs_code_is_refused_before_release_jobs_are_rendered() -> TestResult {
    let mut view = release_view(&[APP], Some("[[binaries]]\npackage = \"app\"\n"))?;
    let text = String::from_utf8(
        view.files
            .get(&RepoPath::new("mise.toml")?)
            .ok_or("no mise.toml")?
            .clone(),
    )?;
    view.files.insert(
        RepoPath::new("mise.toml")?,
        format!("{text}\n[env]\n_.source = \"./payload.sh\"\n").into_bytes(),
    );
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Detect(_))
    ));
    Ok(())
}

#[test]
fn a_released_package_that_is_not_a_member_of_its_cargo_root_is_refused() -> TestResult {
    let mut view = release_view(&[APP], Some("[[binaries]]\npackage = \"app\"\n"))?;
    let metadata = view
        .rust_metadata
        .get_mut(&RepoPath::new("Cargo.toml")?)
        .ok_or("no metadata")?;
    metadata.members[0].name = PackageName::new("other")?;
    assert_eq!(
        expected_files(&view).err(),
        Some(GenerateError::ReleasePackageNotInRoot {
            root: ".".to_owned(),
            package: "app".to_owned()
        })
    );
    Ok(())
}
