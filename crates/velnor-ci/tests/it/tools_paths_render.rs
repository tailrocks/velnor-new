// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use velnor_ci::tools_verify_step::tools_paths;
use velnor_model::{CacheKey, Host, StepKind, ToolsAnchor, WORKFLOW_PATH};
use velnor_render::render;

use crate::sources_fetch_contract::plan;

fn rendered_tools_paths() -> Vec<String> {
    let mut plan = plan(
        &["Cargo.toml"],
        "ubuntu-latest",
        &["ubuntu-latest"],
        velnor_model::TrustedRunners::default(),
    );
    plan.jobs[0].steps[0].kind = StepKind::CacheRestore(CacheKey::Tools {
        host: Host,
        anchor: ToolsAnchor::Unanchored,
    });
    let files = render(&plan).unwrap_or_else(|e| unreachable!("{e:?}"));
    let text = files
        .get(&WORKFLOW_PATH)
        .unwrap_or_else(|| unreachable!("no workflow"));
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim_start().starts_with("path:"))
        .unwrap_or_else(|| unreachable!("no path in {text}"));
    let indent = lines[start].len() - lines[start].trim_start().len();
    lines[start + 1..]
        .iter()
        .take_while(|line| line.len() - line.trim_start().len() > indent)
        .map(|line| line.trim().to_owned())
        .collect()
}

#[test]
fn the_paths_the_notice_versions_are_the_paths_the_rendered_tools_cache_stores() {
    assert_eq!(
        rendered_tools_paths(),
        tools_paths().unwrap_or_else(|e| unreachable!("{e}"))
    );
}

#[test]
fn the_notice_versions_the_two_tools_directories_and_nothing_else() {
    assert_eq!(
        tools_paths().unwrap_or_else(|e| unreachable!("{e}")),
        ["~/.local/share/mise", "~/.rustup"]
    );
}
