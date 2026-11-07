// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use velnor_actions_workflow_renderer::owned_tool_publication::{
    OwnedBuildBootstrap, OwnedPublicationSpec, SourceBuildBinding, SourceQualificationTrigger,
    render_owned_publication_files,
};

#[test]
fn owned_publication_requires_pinned_ubuntu_26_bootstraps() -> Result<(), Box<dyn Error>> {
    let mut spec = specification();
    let files = render_owned_publication_files(&spec)?;
    let (build, qualify) = files[0]
        .bytes
        .split_once("\n  qualify:\n")
        .ok_or("missing owned publication qualifier")?;
    for job in [build, qualify] {
        let fields: Vec<_> = job.lines().map(str::trim).collect();
        assert!(fields.windows(2).any(|pair| {
            pair == [
                "target: aarch64-unknown-linux-gnu",
                "runner: ubuntu-26.04-arm",
            ]
        }));
    }
    spec.bootstraps[1].runner = "ubuntu-24.04-arm".to_owned();
    assert!(render_owned_publication_files(&spec).is_err());
    Ok(())
}

fn specification() -> OwnedPublicationSpec {
    OwnedPublicationSpec {
        trigger: SourceQualificationTrigger::DefaultBranchDispatch,
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        checkout_uses: "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683".to_owned(),
        sources: vec![SourceBuildBinding {
            label: "mise".to_owned(),
            source_json: "{}".to_owned(),
        }],
        bootstraps: [
            ("x86_64-unknown-linux-gnu", "ubuntu-26.04"),
            ("aarch64-unknown-linux-gnu", "ubuntu-26.04-arm"),
            ("aarch64-apple-darwin", "macos-26"),
        ]
        .into_iter()
        .map(|(target, runner)| OwnedBuildBootstrap {
            target: target.to_owned(),
            runner: runner.to_owned(),
            assets_json: "{}".to_owned(),
        })
        .collect(),
    }
}
