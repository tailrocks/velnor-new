// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoPath;
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, Root, release_plz, release_view, render};
use crate::release_yml_binaries::job;
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const PUBLISHING: Root = Root {
    publishes: true,
    ..APP
};

fn with_timeout(value: &str) -> Result<Result<Node, GenerateError>, Box<dyn Error>> {
    let mut view = release_view(&[PUBLISHING], Some("roots = [\".\"]\n"))?;
    let text = release_plz(&PUBLISHING).replace(
        "[workspace]\n",
        &format!("[workspace]\npublish_timeout = \"{value}\"\n"),
    );
    view.files
        .insert(RepoPath::new("release-plz.toml")?, text.into_bytes());
    match expected_files(&view) {
        Ok(mut files) => {
            let text = files
                .remove(&velnor_model::RELEASE_WORKFLOW_PATH)
                .ok_or("no release.yml")?;
            Ok(Ok(crate::yaml_tree::parse(&text)?))
        }
        Err(error) => Ok(Err(error)),
    }
}

#[test]
fn the_tag_job_is_sized_for_the_publish_timeout_of_every_published_package() -> TestResult {
    let (_, default) = render(&[PUBLISHING], Some("roots = [\".\"]\n"))?;
    assert_eq!(
        job(&default, "tag-0")?
            .get("timeout-minutes")
            .and_then(Node::text),
        Some("30")
    );
    for (value, minutes) in [("45m", "75"), ("1h 30m", "120"), ("90s", "32")] {
        let tree = with_timeout(value)??;
        assert_eq!(
            job(&tree, "tag-0")?
                .get("timeout-minutes")
                .and_then(Node::text),
            Some(minutes),
            "{value}"
        );
    }
    Ok(())
}

#[test]
fn a_publish_timeout_the_tag_job_cannot_honour_or_that_is_not_a_duration_is_refused() -> TestResult
{
    assert!(matches!(
        with_timeout("400m")?.err(),
        Some(GenerateError::Release(
            ReleaseError::PublishTimeoutTooLong { minutes: 430, .. }
        ))
    ));
    assert!(matches!(
        with_timeout("soon")?.err(),
        Some(GenerateError::Release(
            ReleaseError::InvalidPublishTimeout { .. }
        ))
    ));
    Ok(())
}

#[test]
fn a_release_workflow_above_the_github_file_size_limit_is_refused() -> TestResult {
    let roots: Vec<Root> = (0..60)
        .map(|index| Root {
            dir: Box::leak(format!("crate-{index}").into_boxed_str()),
            package: Box::leak(format!("package-{index}").into_boxed_str()),
            prefix: Box::leak(format!("release-plz-{index}-").into_boxed_str()),
            publishes: false,
        })
        .collect();
    let list: Vec<String> = roots
        .iter()
        .map(|root| format!("\"{}\"", root.dir))
        .collect();
    let binaries: Vec<String> = roots
        .iter()
        .map(|root| format!("[[binaries]]\npackage = \"{}\"\n", root.package))
        .collect();
    let config = format!("roots = [{}]\n{}", list.join(", "), binaries.concat());
    let error = expected_files(&release_view(&roots, Some(&config))?).err();
    assert!(matches!(
        error,
        Some(GenerateError::Rules(violations))
            if violations.iter().any(|violation| matches!(violation.rule, velnor_render::Rule::WorkflowTooLarge(_)))
    ));
    Ok(())
}
