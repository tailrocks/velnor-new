// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::release_fixture::steps;
use crate::yaml_tree::Node;

pub(super) const PLATFORMS: [&str; 3] = ["linux-x64", "linux-arm64", "macos-arm64"];

pub(crate) fn job<'a>(tree: &'a Node, id: &str) -> Result<&'a Node, Box<dyn Error>> {
    Ok(tree
        .at(&["jobs", id])
        .ok_or_else(|| format!("no job {id}"))?)
}

pub(crate) fn permissions(job: &Node) -> Vec<(String, String)> {
    job.get("permissions").map_or_else(Vec::new, |node| {
        node.keys()
            .into_iter()
            .filter_map(|key| {
                node.get(key)
                    .and_then(Node::text)
                    .map(|access| (key.to_owned(), access.to_owned()))
            })
            .collect()
    })
}

pub(crate) fn runs(job: &Node) -> Vec<&str> {
    steps(job)
        .iter()
        .filter_map(|step| step.get("run").and_then(Node::text))
        .collect()
}

pub(super) fn triple(platform: &str) -> &'static str {
    match platform {
        "linux-x64" => "x86_64-unknown-linux-musl",
        "linux-arm64" => "aarch64-unknown-linux-musl",
        _ => "aarch64-apple-darwin",
    }
}

pub(super) fn build_ids(package: &str, platforms: &[&str]) -> Vec<String> {
    platforms
        .iter()
        .map(|platform| format!("build-{package}-{platform}"))
        .collect()
}
