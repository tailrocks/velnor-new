// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{RepoPath, RepoView, RequiredFeature, Target, TargetKind, TargetName};
use velnor_rust::detect;

use super::{FEATURES, GATED_HARNESS, MANIFEST, refusal, repository};
use crate::facts::TestResult;
use crate::pipeline::workflow;

const ROOT_WITHOUT_FEATURE: &str = "[package]\nname = \"package-0\"\n";

fn root_and_member(root_manifest: &str, config: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut repo = repository(&["Cargo.toml"], config)?;
    repo.files.insert(
        RepoPath::new("Cargo.toml")?,
        root_manifest.as_bytes().to_vec(),
    );
    let manifest = RepoPath::new("crates/member/Cargo.toml")?;
    repo.tracked_paths.insert(manifest.clone());
    repo.files.insert(manifest.clone(), MANIFEST.to_vec());
    for metadata in repo.rust_metadata.values_mut() {
        let mut member = metadata.members[0].clone();
        member.name = velnor_model::PackageName::new("member")?;
        member.manifest = manifest.clone();
        metadata.members.push(member);
    }
    Ok(repo)
}

#[test]
fn a_plain_name_a_member_alone_defines_is_refused_for_a_root_package_under_resolver_one()
-> TestResult {
    for (edition, resolver_one) in [("2018", true), ("2021", false)] {
        let root = format!("{ROOT_WITHOUT_FEATURE}edition = \"{edition}\"\n");
        let repo = root_and_member(&root, FEATURES)?;
        if resolver_one {
            assert_eq!(
                refusal(&repo)?,
                (".".to_owned(), "local-mode".to_owned()),
                "{edition}"
            );
        } else {
            detect(&repo)?;
        }
    }
    let inherited = |edition: &str| {
        format!(
            "[package]\nname = \"package-0\"\nedition.workspace = true\n[workspace.package]\nedition = \"{edition}\"\n"
        )
    };
    assert_eq!(
        refusal(&root_and_member(&inherited("2018"), FEATURES)?)?,
        (".".to_owned(), "local-mode".to_owned())
    );
    detect(&root_and_member(&inherited("2021"), FEATURES)?)?;
    let explicit = format!("{ROOT_WITHOUT_FEATURE}edition = \"2018\"\nresolver = \"2\"\n");
    detect(&root_and_member(&explicit, FEATURES)?)?;
    let defined =
        format!("{ROOT_WITHOUT_FEATURE}edition = \"2018\"\n[features]\nlocal-mode = []\n");
    detect(&root_and_member(&defined, FEATURES)?)?;
    let virtual_root = "[workspace]\nmembers = [\"crates/member\"]\n";
    detect(&root_and_member(virtual_root, FEATURES)?)?;
    Ok(())
}

fn member_gate(root_manifest: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut repo = root_and_member(root_manifest, FEATURES)?;
    repo.files.insert(
        RepoPath::new("crates/member/Cargo.toml")?,
        GATED_HARNESS.to_vec(),
    );
    for metadata in repo.rust_metadata.values_mut() {
        metadata.members[1].targets.push(Target {
            name: TargetName::new("gate")?,
            kinds: vec![TargetKind::Test],
            tests: true,
            doctest: false,
            source: None,
            requires_features: true,
            required_features: vec![RequiredFeature {
                name: "local-mode".to_owned(),
                enabled_by: vec!["local-mode".to_owned()],
            }],
        });
    }
    Ok(workflow(&repo)?
        .lines()
        .filter(|line| line.contains("--test=gate"))
        .map(str::to_owned)
        .collect())
}

#[test]
fn a_member_harness_gated_on_the_feature_runs_directly_only_when_the_root_package_does_not_confine_the_name()
-> TestResult {
    let root = |edition: &str| {
        format!("{ROOT_WITHOUT_FEATURE}edition = \"{edition}\"\n[features]\nlocal-mode = []\n")
    };
    assert_eq!(member_gate(&root("2018"))?.len(), 0);
    assert_eq!(member_gate(&root("2021"))?.len(), 1);
    let inherited = |edition: &str| {
        format!(
            "[package]\nname = \"package-0\"\nedition.workspace = true\n[workspace.package]\nedition = \"{edition}\"\n[features]\nlocal-mode = []\n"
        )
    };
    assert_eq!(member_gate(&inherited("2018"))?.len(), 0);
    assert_eq!(member_gate(&inherited("2021"))?.len(), 1);
    Ok(())
}
