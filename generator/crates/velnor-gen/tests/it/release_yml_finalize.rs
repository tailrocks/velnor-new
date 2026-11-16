// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::release_fixture::{APP, TOOLS, render, steps, two_binaries};
use crate::release_yml_binaries::job;
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn a_package_without_a_binaries_entry_gets_a_job_that_publishes_its_draft_release() -> TestResult {
    let (_, tree) = render(&[APP], Some("roots = [\".\"]\n"))?;
    let finalize = job(&tree, "finalize-0")?;
    assert_eq!(
        finalize.get("needs").map(Node::texts),
        Some(vec!["gate", "tag-0"])
    );
    assert_eq!(
        finalize
            .at(&["permissions", "contents"])
            .and_then(Node::text),
        Some("write")
    );
    let all = steps(finalize);
    assert_eq!(all.len(), 1);
    let step = &all[0];
    assert_eq!(
        step.get("run").and_then(Node::text),
        Some("gh release edit \"$TAG\" --repo \"$REPOSITORY\" --draft=false")
    );
    assert_eq!(
        step.get("if").and_then(Node::text),
        Some("${{ needs.tag-0.outputs.app != \'\' }}")
    );
    assert_eq!(
        step.at(&["env", "TAG"]).and_then(Node::text),
        Some("${{ needs.tag-0.outputs.app }}")
    );
    assert_eq!(
        job(&tree, "tag-0")?.get("outputs").map(Node::keys),
        Some(vec!["app"])
    );
    Ok(())
}

#[test]
fn packages_with_a_binaries_entry_are_published_by_their_own_job_and_not_finalized() -> TestResult {
    let (_, tree) = two_binaries()?;
    assert!(tree.at(&["jobs", "finalize-0"]).is_none());
    assert!(tree.at(&["jobs", "finalize-1"]).is_none());

    let (_, mixed) = render(
        &[APP, TOOLS],
        Some("roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"tools\"\n"),
    )?;
    assert!(mixed.at(&["jobs", "finalize-0"]).is_some());
    assert!(mixed.at(&["jobs", "finalize-1"]).is_none());
    assert_eq!(
        job(&mixed, "tag-1")?.get("outputs").map(Node::keys),
        Some(vec!["tools"])
    );
    Ok(())
}
