// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use velnor_actions_workflow_renderer::{
    owned_tool_publication::{
        OwnedBuildBootstrap, OwnedPublicationSpec, SourceBuildBinding, SourceQualificationTrigger,
        render_owned_publication_files,
    },
    steps::CHECKOUT_USES,
};

#[test]
fn owned_publication_uses_only_ubuntu_26_for_linux() -> Result<(), Box<dyn Error>> {
    let mut spec = OwnedPublicationSpec {
        trigger: SourceQualificationTrigger::DefaultBranchDispatch,
        generator_version: "0.1.75".to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        sources: vec![SourceBuildBinding {
            label: "fixture".to_owned(),
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
            assets_json: "[]".to_owned(),
        })
        .collect(),
    };
    spec.validate()?;
    let files = render_owned_publication_files(&spec)?;
    let workflow = files[0].bytes.as_str();
    assert!(workflow.contains("ubuntu-26.04"));
    assert!(workflow.contains("ubuntu-26.04-arm"));
    assert!(!workflow.contains("ubuntu-24.04"));
    spec.bootstraps[1].runner = "ubuntu-24.04-arm".to_owned();
    assert!(spec.validate().is_err());
    assert!(render_owned_publication_files(&spec).is_err());
    Ok(())
}
