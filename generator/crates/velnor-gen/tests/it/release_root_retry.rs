// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RELEASE_WORKFLOW_PATH;

use crate::release_fixture::{APP, LOCK, TOOLS, release_view, steps};
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;
type Step = (String, String, String);

fn names(version: &str) -> Result<Vec<Step>, Box<dyn Error>> {
    let mut view = release_view(
        &[APP, TOOLS],
        Some("roots = [\".\", \"tools\"]\nafter = [\"Rust CI\", \"Lint\"]\n"),
    )?;
    view.files.insert(
        velnor_rust::LOCK_PATH,
        LOCK.replace("0.1.79", version).into_bytes(),
    );
    let text = expected_files(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    let tree = parse(&text)?;
    let job = tree.at(&["jobs", "tag"]).ok_or("no tag job")?;
    Ok(steps(job)
        .iter()
        .map(|step| {
            let text = |key: &str| {
                step.get(key)
                    .and_then(Node::text)
                    .unwrap_or_default()
                    .to_owned()
            };
            (text("name"), text("run"), text("if"))
        })
        .collect())
}

#[test]
fn every_root_looks_up_its_tags_before_the_token_and_release_plz_waits_for_it() -> TestResult {
    let all = names("0.1.82")?;
    let token = all
        .iter()
        .position(|(name, _, _)| name == "Create the release App token")
        .ok_or("no token")?;
    let lookups: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, (_, run, _))| run.contains("release root-tags"))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(lookups.len(), 2);
    assert!(lookups.iter().all(|index| *index < token));
    let tagging: Vec<&Step> = all
        .iter()
        .filter(|(_, run, _)| run.contains("release-plz\" release"))
        .collect();
    assert_eq!(tagging.len(), 2);
    for (index, (_, _, condition)) in tagging.iter().enumerate() {
        assert!(
            condition.contains(&format!("steps.tags-{index}.outputs.done != 'true'")),
            "{condition}"
        );
    }
    let record = all
        .iter()
        .find(|(_, run, _)| run.contains("release record"))
        .ok_or("no record")?;
    assert!(record.1.contains("tagged-0.json") && record.1.contains("tagged-1.json"));
    assert!(record.1.contains("release-0.json") && record.1.contains("release-1.json"));
    Ok(())
}

#[test]
fn a_helper_without_root_tags_renders_the_tag_job_as_before() -> TestResult {
    let all = names("0.1.81")?;
    assert!(
        all.iter()
            .all(|(_, run, condition)| !run.contains("root-tags") && !condition.contains("tags-"))
    );
    let record = all
        .iter()
        .find(|(_, run, _)| run.contains("release record"))
        .ok_or("no record")?;
    assert!(!record.1.contains("tagged-"));
    Ok(())
}
