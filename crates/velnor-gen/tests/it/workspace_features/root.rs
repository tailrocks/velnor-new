// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{RepoPath, RepoView};
use velnor_rust::detect;

use super::{FEATURES, MANIFEST, refusal, repository};
use crate::facts::TestResult;

const ROOT_WITHOUT_FEATURE: &str = "[package]\nname = \"package-0\"\n";

pub(super) fn root_and_member(
    root_manifest: &str,
    config: &str,
) -> Result<RepoView, Box<dyn Error>> {
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
