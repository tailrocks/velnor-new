// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::{expected_files, expected_with_notes};
use velnor_model::RELEASE_WORKFLOW_PATH;
use velnor_rust::LOCK_PATH;

use crate::release_fixture::{APP, LOCK, Root, TOOLS, release_view, steps};
use crate::release_yml_binaries::{job, permissions};
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;

const APP_SLUG: &str = "velnor-release";

fn render_with(helper: &str, roots: &[Root], config: &str) -> Result<Node, Box<dyn Error>> {
    let mut view = release_view(roots, Some(config))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.79", helper).into_bytes());
    let text = expected_files(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    Ok(parse(&text)?)
}

#[test]
fn a_package_without_a_binaries_entry_is_published_by_the_guarded_helper_command() -> TestResult {
    let tree = render_with("0.1.81", &[APP], "roots = [\".\"]\n")?;
    let finalize = job(&tree, "finalize")?;
    assert_eq!(
        finalize.get("needs").map(Node::texts),
        Some(vec!["gate", "tag"])
    );
    assert_eq!(
        permissions(finalize),
        [("contents".to_owned(), "read".to_owned())]
    );
    let all = steps(finalize);
    let token = all
        .iter()
        .position(|step| {
            step.get("uses")
                .and_then(Node::text)
                .is_some_and(|uses| uses.starts_with("actions/create-github-app-token@"))
        })
        .ok_or("no App token")?;
    assert_eq!(
        all[token]
            .at(&["with", "permission-contents"])
            .and_then(Node::text),
        Some("write")
    );
    let publish = &all[all.len() - 2];
    assert_eq!(
        publish.get("run").and_then(Node::text),
        Some(
            format!(
                "velnor-ci release publish --repo \"$REPOSITORY\" --tag \"$TAG\" --app {APP_SLUG} --no-files"
            )
            .as_str()
        )
    );
    assert_eq!(
        publish.at(&["env", "GITHUB_TOKEN"]).and_then(Node::text),
        Some("${{ steps.app.outputs.token }}")
    );
    assert_eq!(
        publish.at(&["env", "TAG"]).and_then(Node::text),
        Some("${{ needs.tag.outputs.app }}")
    );
    assert!(all.len() > token + 1);
    for step in all {
        let run = step.get("run").and_then(Node::text).unwrap_or_default();
        assert!(!run.contains("gh release"), "{run}");
    }
    assert_eq!(
        job(&tree, "tag")?.get("outputs").map(Node::keys),
        Some(vec!["app"])
    );
    Ok(())
}

#[test]
fn the_finalizer_runs_only_when_a_package_without_a_binary_was_released() -> TestResult {
    let tree = render_with("0.1.81", &[APP, TOOLS], "roots = [\".\", \"tools\"]\n")?;
    let finalize = job(&tree, "finalize")?;
    let condition = finalize.get("if").and_then(Node::text).ok_or("no if")?;
    assert!(
        condition
            .ends_with(" && (needs.tag.outputs.app != '' || needs.tag.outputs.tools != '') }}"),
        "{condition}"
    );
    assert_eq!(
        finalize.get("environment").and_then(Node::text),
        job(&tree, "tag")?.get("environment").and_then(Node::text)
    );
    Ok(())
}

#[test]
fn packages_with_a_binaries_entry_are_not_finalized() -> TestResult {
    let tree = render_with(
        "0.1.81",
        &[APP, TOOLS],
        "roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\n[[binaries]]\npackage = \"tools\"\n",
    )?;
    assert!(tree.at(&["jobs", "finalize"]).is_none());
    let mixed = render_with(
        "0.1.81",
        &[APP, TOOLS],
        "roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"tools\"\n",
    )?;
    let condition = job(&mixed, "finalize")?
        .get("if")
        .and_then(Node::text)
        .ok_or("no if")?;
    assert!(
        condition.ends_with(" && (needs.tag.outputs.app != '') }}"),
        "{condition}"
    );
    assert_eq!(
        job(&mixed, "tag")?.get("outputs").map(Node::keys),
        Some(vec!["tools", "app"])
    );
    Ok(())
}

#[test]
fn a_helper_that_cannot_publish_an_assetless_release_gets_no_finalizer() -> TestResult {
    for version in ["0.1.80", "0.1.79"] {
        let tree = render_with(version, &[APP], "roots = [\".\"]\n")?;
        assert!(tree.at(&["jobs", "finalize"]).is_none(), "{version}");
        assert!(job(&tree, "tag")?.get("outputs").is_none(), "{version}");
    }
    Ok(())
}

fn notes_with(helper: &str, config: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut view = release_view(&[APP], Some(config))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.79", helper).into_bytes());
    Ok(expected_with_notes(&view)?.1)
}

#[test]
fn an_old_helper_with_an_assetless_package_warns_naming_the_version_to_pin() -> TestResult {
    for version in ["0.1.80", "0.1.79"] {
        let notes = notes_with(version, "roots = [\".\"]\n")?;
        assert_eq!(notes.len(), 1, "{version}: {notes:?}");
        assert!(notes[0].starts_with("WARNING: "), "{notes:?}");
        assert!(notes[0].contains(version), "{notes:?}");
        assert!(notes[0].contains("app"), "{notes:?}");
        assert!(notes[0].contains("0.1.81"), "{notes:?}");
    }
    assert_eq!(
        notes_with("0.1.81", "roots = [\".\"]\n")?,
        Vec::<String>::new()
    );
    let binary = "roots = [\".\"]\n[[binaries]]\npackage = \"app\"\n";
    assert_eq!(notes_with("0.1.80", binary)?, Vec::<String>::new());
    Ok(())
}

#[test]
fn a_failed_finalization_does_not_skip_the_others_and_still_fails_the_job() -> TestResult {
    let tree = render_with("0.1.81", &[APP, TOOLS], "roots = [\".\", \"tools\"]\n")?;
    let all = steps(job(&tree, "finalize")?);
    let publishing: Vec<&Node> = all
        .iter()
        .filter(|step| {
            step.get("run")
                .and_then(Node::text)
                .is_some_and(|run| run.contains("release publish"))
        })
        .collect();
    assert_eq!(publishing.len(), 2);
    for (index, step) in publishing.iter().enumerate() {
        assert_eq!(
            step.get("continue-on-error").and_then(Node::text),
            Some("true")
        );
        assert_eq!(
            step.get("id").and_then(Node::text),
            Some(format!("publish_{index}").as_str())
        );
    }
    let last = all.last().ok_or("no steps")?;
    assert_eq!(last.get("run").and_then(Node::text), Some("exit 1"));
    assert_eq!(
        last.get("if").and_then(Node::text),
        Some("${{ steps.publish_0.outcome == 'failure' || steps.publish_1.outcome == 'failure' }}")
    );
    Ok(())
}

#[test]
fn the_finalize_timeout_grows_with_the_assetless_packages_by_five_requests_of_five_minutes_each()
-> TestResult {
    let one = render_with("0.1.81", &[APP], "roots = [\".\"]\n")?;
    assert_eq!(
        job(&one, "finalize")?
            .get("timeout-minutes")
            .and_then(Node::text),
        Some("30")
    );
    let two = render_with("0.1.81", &[APP, TOOLS], "roots = [\".\", \"tools\"]\n")?;
    assert_eq!(
        job(&two, "finalize")?
            .get("timeout-minutes")
            .and_then(Node::text),
        Some("50")
    );
    Ok(())
}

const DOCS: Root = Root {
    dir: "docs",
    package: "docs",
    prefix: "release-plz-docs-",
    publishes: false,
};

#[test]
fn the_finalize_timeout_is_capped_at_fifty_minutes_so_it_cannot_outlive_its_one_hour_app_token()
-> TestResult {
    let three = render_with(
        "0.1.81",
        &[APP, TOOLS, DOCS],
        "roots = [\".\", \"tools\", \"docs\"]\n",
    )?;
    let minutes: u16 = job(&three, "finalize")?
        .get("timeout-minutes")
        .and_then(Node::text)
        .ok_or("no timeout")?
        .parse()?;
    assert!(minutes <= 50, "{minutes} for three packages");
    assert_eq!(minutes, 50);
    Ok(())
}
