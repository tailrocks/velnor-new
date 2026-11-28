// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::private_fill_roots::{
    Layout, PULL_REQUEST, PUSH, RELEASE, Run, every_root_on, every_root_on_a_saving_run, fetched,
    grouped_roots_at, lanes, own_roots, quoted_long_roots, selecting,
};
use crate::private_sources::{SECTION, private_repo};

fn the_member_lane(
    layout: &Layout,
    run: Run,
) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let mut members = Vec::new();
    for steps in lanes(layout)? {
        for fetched in fetched(&steps, run)? {
            if fetched
                .iter()
                .any(|manifest| manifest.starts_with("tools/"))
            {
                members.push(fetched);
            }
        }
    }
    assert_eq!(members.len(), 1, "{members:?}");
    Ok(members.remove(0))
}

fn set(manifests: &[&str]) -> BTreeSet<String> {
    manifests
        .iter()
        .map(|manifest| (*manifest).to_owned())
        .collect()
}

#[test]
fn an_oversized_credentialed_fetch_lists_roots_whose_paths_are_not_plain_words() -> TestResult {
    let layout = quoted_long_roots("0.1.116");
    every_root_on_a_saving_run(&layout)?;
    own_roots(&layout, PULL_REQUEST)
}

#[test]
fn a_helper_without_the_manifest_list_still_refuses_an_oversized_fetch_it_cannot_split()
-> TestResult {
    let layout = quoted_long_roots("0.1.115");
    let names: Vec<&str> = layout.roots.iter().map(String::as_str).collect();
    let repo = private_repo(&names, &layout.config, layout.helper)?;
    assert!(workflow(&repo).is_err());
    Ok(())
}

#[test]
fn a_pull_request_fetches_only_the_grouped_members_it_selects() -> TestResult {
    let layout = grouped_roots_at("0.1.116");
    let both = set(&["tools/a/Cargo.toml", "tools/b/Cargo.toml"]);
    assert_eq!(the_member_lane(&layout, PULL_REQUEST)?, both);
    assert_eq!(
        the_member_lane(&layout, selecting(PULL_REQUEST, "tools_a"))?,
        set(&["tools/a/Cargo.toml"])
    );
    assert_eq!(
        the_member_lane(&layout, selecting(PULL_REQUEST, "tools_b"))?,
        set(&["tools/b/Cargo.toml"])
    );
    Ok(())
}

#[test]
fn a_run_on_the_default_branch_fetches_every_grouped_member_whatever_it_selects() -> TestResult {
    let layout = grouped_roots_at("0.1.116");
    every_root_on(&layout, selecting(PUSH, "tools_a"))?;
    assert_eq!(
        the_member_lane(&layout, selecting(RELEASE, "tools_a"))?,
        set(&["tools/a/Cargo.toml", "tools/b/Cargo.toml"])
    );
    Ok(())
}

#[test]
fn a_helper_before_the_manifest_list_fetches_every_grouped_member_on_a_pull_request() -> TestResult
{
    let layout = grouped_roots_at("0.1.115");
    assert_eq!(
        the_member_lane(&layout, selecting(PULL_REQUEST, "tools_a"))?,
        set(&["tools/a/Cargo.toml", "tools/b/Cargo.toml"])
    );
    Ok(())
}

fn many_long_members() -> Layout {
    let folders: Vec<String> = (0..34)
        .map(|index| format!("crates/{}{index:02}", "x".repeat(148)))
        .collect();
    let quoted: Vec<String> = folders
        .iter()
        .map(|folder| format!("\"{folder}\""))
        .collect();
    Layout {
        roots: folders
            .iter()
            .map(|folder| format!("{folder}/Cargo.toml"))
            .collect(),
        config: format!(
            "[[workspace_groups]]\nname = 'members'\nroots = [{}]\n{SECTION}",
            quoted.join(", ")
        ),
        locked: false,
        lanes: 1,
        helper: "0.1.116",
    }
}

#[test]
fn a_member_list_too_long_for_a_github_expression_is_refused_for_its_step_condition() {
    let refused = lanes(&many_long_members())
        .err()
        .map(|error| format!("{error:?}"));
    assert!(
        refused.is_some_and(|error| error.contains("ConditionTooLong")),
        "generation must refuse a step condition over the limit"
    );
}
