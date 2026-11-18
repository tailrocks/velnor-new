// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::{GenerateError, expected_files};
use velnor_release::{CheckProblem, ReleaseError};
use velnor_render::{Rule, RuleViolation};

use crate::release_fixture::{APP, TOOLS, jobs, release_view, render, steps, uses};
use crate::release_yml_binaries::{job, permissions};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const BINARY: &str =
    "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\n";

fn config(checks: &str) -> String {
    format!("{BINARY}\n{checks}")
}

fn refusal(checks: &str) -> Result<Option<GenerateError>, Box<dyn Error>> {
    Ok(expected_files(&release_view(&[APP], Some(&config(checks)))?).err())
}

fn problem(checks: &str) -> Result<Option<CheckProblem>, Box<dyn Error>> {
    Ok(match refusal(checks)? {
        Some(GenerateError::Release(ReleaseError::InvalidCheck(_, problem))) => Some(problem),
        _ => None,
    })
}

const BEFORE_TAG: &str =
    "[[checks]]\nname = \"Lint the repository\"\nrun = \"bash scripts/lint.sh\"\n";
const BEFORE_PUBLISH: &str = "[[checks]]\nname = \"Qualify linux\"\nrun = \"bash scripts/qualify.sh\"\nbefore = \"publish\"\nartifacts = [\"app-linux-x64\"]\n";

#[test]
fn a_check_runs_after_the_gate_with_read_only_contents_and_blocks_the_tag() -> TestResult {
    let (_, tree) = render(&[APP], Some(&config(BEFORE_TAG)))?;
    let check = job(&tree, "check-lint-the-repository")?;
    assert_eq!(check.get("needs").map(Node::texts), Some(vec!["gate"]));
    assert_eq!(
        permissions(check),
        [("contents".to_owned(), "read".to_owned())]
    );
    assert!(check.get("environment").is_none());
    let all = steps(check);
    assert!(uses(&all[0]).is_some_and(|name| name.starts_with("actions/checkout@")));
    assert_eq!(
        all.last()
            .and_then(|step| step.get("run"))
            .and_then(Node::text),
        Some("bash scripts/lint.sh")
    );
    let tag = job(&tree, "tag")?;
    assert_eq!(
        tag.get("needs").map(Node::texts),
        Some(vec!["gate", "check-lint-the-repository"])
    );
    Ok(())
}

#[test]
fn a_failing_check_leaves_every_publishing_job_unreachable() -> TestResult {
    let both = format!("{BEFORE_TAG}\n{BEFORE_PUBLISH}");
    let (_, tree) = render(&[APP], Some(&config(&both)))?;
    let qualify = job(&tree, "check-qualify-linux")?;
    let needs = qualify.get("needs").map(Node::texts).ok_or("no needs")?;
    assert!(needs.contains(&"tag") && needs.contains(&"build-app-linux-x64"));
    assert!(!needs.contains(&"build-app-macos-arm64"));
    let download = steps(qualify)
        .iter()
        .find(|step| uses(step).is_some_and(|name| name.starts_with("actions/download-artifact@")))
        .ok_or("no download")?;
    assert_eq!(
        download.at(&["with", "name"]).and_then(Node::text),
        Some("app-linux-x64")
    );
    let publish = job(&tree, "publish-app")?;
    assert!(
        publish
            .get("needs")
            .map(Node::texts)
            .ok_or("no needs")?
            .contains(&"check-qualify-linux")
    );
    Ok(())
}

#[test]
fn a_check_adds_only_the_permissions_it_declares() -> TestResult {
    let declared = "[[checks]]\nname = \"Needs runs\"\nrun = \"bash scripts/a.sh\"\n[checks.permissions]\nactions = \"read\"\n";
    let (_, tree) = render(&[APP], Some(&config(declared)))?;
    assert_eq!(
        permissions(job(&tree, "check-needs-runs")?),
        [
            ("actions".to_owned(), "read".to_owned()),
            ("contents".to_owned(), "read".to_owned())
        ]
    );
    Ok(())
}

#[test]
fn checks_chain_through_needs_and_pick_their_runner() -> TestResult {
    let chain = "[[checks]]\nname = \"one\"\nrun = \"bash a.sh\"\nruns_on = \"macos-15\"\n[[checks]]\nname = \"two\"\nrun = \"bash b.sh\"\nneeds = [\"one\"]\n";
    let (_, tree) = render(&[APP], Some(&config(chain)))?;
    assert_eq!(
        job(&tree, "check-two")?.get("needs").map(Node::texts),
        Some(vec!["gate", "check-one"])
    );
    assert_eq!(
        job(&tree, "check-one")?.get("runs-on").and_then(Node::text),
        Some("macos-15")
    );
    Ok(())
}

#[test]
fn a_repository_without_checks_renders_the_same_workflow() -> TestResult {
    let (plain, tree) = render(&[APP], Some(BINARY))?;
    let (empty, _) = render(&[APP], Some(&format!("checks = []\n{BINARY}")))?;
    assert_eq!(plain, empty);
    assert!(jobs(&tree)?.iter().all(|(id, _)| !id.starts_with("check-")));
    Ok(())
}

#[test]
fn bad_checks_are_refused() -> TestResult {
    let cases: [(&str, CheckProblem); 10] = [
        (
            "[[checks]]\nname = \"a\"\nrun = \"x\"\n[[checks]]\nname = \"A\"\nrun = \"y\"\n",
            CheckProblem::DuplicateName,
        ),
        (
            "[[checks]]\nname = \"a\"\nrun = \"x\"\nneeds = [\"later\"]\n",
            CheckProblem::UnknownNeed("later".to_owned()),
        ),
        (
            "[[checks]]\nname = \"a\"\nrun = \"x\"\nbefore = \"publish\"\n",
            CheckProblem::PublishWithoutArtifacts,
        ),
        (
            "[[checks]]\nname = \"a\"\nrun = \"x\"\nbefore = \"publish\"\nartifacts = [\"app-linux-arm64\"]\n",
            CheckProblem::UnknownArtifact("app-linux-arm64".to_owned()),
        ),
        (
            "[[checks]]\nname = \"a\"\nrun = \"x\"\nartifacts = [\"app-linux-x64\"]\n",
            CheckProblem::ArtifactsBeforeTag,
        ),
        (
            "[[checks]]\nname = \"a\"\nrun = \"x\"\n[checks.permissions]\ncontents = \"write\"\n",
            CheckProblem::WritesContents,
        ),
        (
            "[[checks]]\nname = \"a\"\nrun = \"x\"\n[checks.permissions]\nbogus = \"read\"\n",
            CheckProblem::UnknownPermission("bogus".to_owned()),
        ),
        (
            "[[checks]]\nname = \"!!\"\nrun = \"x\"\n",
            CheckProblem::Empty("name"),
        ),
        (
            "[[checks]]\nname = \"a\"\nrun = \"x\"\ntimeout_minutes = 0\n",
            CheckProblem::TimeoutOutOfRange,
        ),
        (
            "[[checks]]\nname = \"a\"\nrun = \"x\"\nbefore = \"publish\"\nartifacts = [\"app-linux-x64\"]\n[[checks]]\nname = \"b\"\nrun = \"y\"\nneeds = [\"a\"]\n",
            CheckProblem::NeedsALaterStage("a".to_owned()),
        ),
    ];
    for (checks, expected) in cases {
        assert_eq!(problem(checks)?, Some(expected), "{checks}");
    }
    Ok(())
}

#[test]
fn a_run_value_with_shell_logic_or_an_expression_is_refused_by_the_render_rules() -> TestResult {
    for run in ["a && b", "echo ${{ github.sha }}", "a\\nb"] {
        let checks = format!("[[checks]]\nname = \"a\"\nrun = \"{run}\"\n");
        assert!(matches!(
            refusal(&checks)?,
            Some(GenerateError::Rules(violations))
                if violations.iter().any(|RuleViolation { rule, .. }| matches!(
                    rule,
                    Rule::ForbiddenSyntax(_) | Rule::ExpressionInRun | Rule::MultiLineRun
                ))
        ));
    }
    Ok(())
}

#[test]
fn checks_work_for_a_second_root_binary() -> TestResult {
    let both = "roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"tools\"\n[[checks]]\nname = \"q\"\nrun = \"bash q.sh\"\nbefore = \"publish\"\nartifacts = [\"tools-linux-x64\"]\n";
    let (_, tree) = render(&[APP, TOOLS], Some(both))?;
    let publish = job(&tree, "publish-tools")?;
    assert!(
        publish
            .get("needs")
            .map(Node::texts)
            .ok_or("no needs")?
            .contains(&"check-q")
    );
    Ok(())
}
