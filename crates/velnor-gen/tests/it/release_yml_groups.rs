// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_model::{RELEASE_WORKFLOW_PATH, RepoPath};
use velnor_rust::LOCK_PATH;

use crate::release_fixture::{LOCK, Root, release_view};
use crate::release_unguarded::expected_release;
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;

const TOOLS: Root = Root {
    dir: ".",
    package: "velnor-ci",
    prefix: "release-plz-",
    publishes: false,
};

const GROUPED: &str = "[workspace]\nrelease = false\nrelease_always = false\nsemver_check = false\ngit_only = true\ngit_release_draft = true\npr_branch_prefix = \"release-plz-\"\n\n[[package]]\nname = \"velnor-ci\"\nrelease = true\ngit_tag_name = \"tools-v{{ version }}\"\nversion_group = \"tools\"\n\n[[package]]\nname = \"velnor-gen\"\nrelease = true\ngit_tag_name = \"tools-v{{ version }}\"\nversion_group = \"tools\"\ngit_tag_enable = false\ngit_release_enable = false\n";
const PLAIN: &str = "[workspace]\nrelease = false\nrelease_always = false\nsemver_check = false\ngit_only = true\ngit_release_draft = true\npr_branch_prefix = \"release-plz-\"\n\n[[package]]\nname = \"velnor-ci\"\nrelease = true\ngit_tag_name = \"velnor-ci-v{{ version }}\"\n";

fn render(plz: &str, helper: &str) -> Result<Result<Node, GenerateError>, Box<dyn Error>> {
    let mut view = release_view(&[TOOLS], Some("roots = [\".\"]\n"))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.81", helper).into_bytes());
    view.files
        .insert(RepoPath::new("release-plz.toml")?, plz.as_bytes().to_vec());
    let metadata = view
        .rust_metadata
        .get_mut(&RepoPath::new("Cargo.toml")?)
        .ok_or("no metadata")?;
    let mut second = metadata.members[0].clone();
    second.name = velnor_model::PackageName::new("velnor-gen")?;
    metadata.members.push(second);
    match expected_release(&view) {
        Ok(mut files) => {
            let text = files
                .remove(&RELEASE_WORKFLOW_PATH)
                .ok_or("no release.yml")?;
            Ok(Ok(parse(&text)?))
        }
        Err(error) => Ok(Err(error)),
    }
}

fn record(tree: &Node) -> Result<String, Box<dyn Error>> {
    runs(job(tree, "tag")?)
        .into_iter()
        .find(|command| command.contains("velnor-ci release record"))
        .map(str::to_owned)
        .ok_or_else(|| "no record step".into())
}

#[test]
fn a_version_group_gives_the_record_step_the_release_plz_config_to_read_it_from() -> TestResult {
    let tree = render(GROUPED, "0.1.97")??;
    assert!(record(&tree)?.contains("--version-groups=release-plz.toml"));
    Ok(())
}

#[test]
fn a_helper_before_0_1_97_cannot_record_a_version_group() -> TestResult {
    for helper in ["0.1.96", "0.1.93"] {
        assert!(
            matches!(
                render(GROUPED, helper)?,
                Err(GenerateError::VersionGroupsHelperTooOld(_))
            ),
            "{helper}"
        );
    }
    Ok(())
}

#[test]
fn without_a_version_group_the_record_step_and_the_old_helpers_stay_as_they_were() -> TestResult {
    for helper in ["0.1.96", "0.1.97"] {
        let tree = render(PLAIN, helper)??;
        assert!(!record(&tree)?.contains("--version-groups"), "{helper}");
    }
    Ok(())
}
