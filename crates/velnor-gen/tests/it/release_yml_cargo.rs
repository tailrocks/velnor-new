// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::release_fixture::{SPACED, render, steps, two_binaries};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

fn job<'a>(tree: &'a Node, id: &str) -> Result<&'a Node, Box<dyn Error>> {
    Ok(tree
        .at(&["jobs", id])
        .ok_or_else(|| format!("no job {id}"))?)
}

fn runs(job: &Node) -> Vec<&str> {
    steps(job)
        .iter()
        .filter_map(|step| step.get("run").and_then(Node::text))
        .collect()
}

#[test]
fn release_yml_runs_cargo_from_the_release_root_with_a_fixed_target_directory_and_the_musl_compiler()
-> TestResult {
    let (_, tree) = two_binaries()?;
    let build = job(&tree, "build-tools-linux-x64")?;
    let cargo = steps(build)
        .iter()
        .find(|step| {
            step.get("run")
                .and_then(Node::text)
                .is_some_and(|run| run.starts_with("cargo "))
        })
        .ok_or("no cargo step")?;
    assert_eq!(
        cargo.get("working-directory").and_then(Node::text),
        Some("tools")
    );
    assert_eq!(
        cargo.at(&["env", "TARGET_CC"]).and_then(Node::text),
        Some("musl-gcc")
    );
    let mac = job(&tree, "build-tools-macos-arm64")?;
    assert!(!runs(mac).iter().any(|run| run.contains("apt-get")),);
    let (_, root) = two_binaries()?;
    let at_root = steps(job(&root, "build-app-linux-x64")?)
        .iter()
        .find(|step| {
            step.get("run")
                .and_then(Node::text)
                .is_some_and(|r| r.starts_with("cargo "))
        })
        .ok_or("no cargo step")?;
    assert!(at_root.get("working-directory").is_none());
    Ok(())
}

#[test]
fn release_yml_quotes_a_root_with_a_space_in_the_build_commands() -> TestResult {
    let (_, tree) = render(
        &[SPACED],
        Some(
            "roots = [\"my tools\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n",
        ),
    )?;
    assert_eq!(
        runs(job(&tree, "build-tools-linux-x64")?),
        [
            "test -n \"$COMMIT\" -a \"$CHECKED_OUT\" = \"$COMMIT\"",
            "sudo apt-get install --yes musl-tools",
            "rustup target add x86_64-unknown-linux-musl",
            "cargo build --release --locked --manifest-path Cargo.toml --package tools --bin tools --target x86_64-unknown-linux-musl --target-dir target",
            "tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -czf tools-x86_64-unknown-linux-musl.tar.gz -C 'my tools/target/x86_64-unknown-linux-musl/release' tools",
        ]
    );
    Ok(())
}

#[test]
fn an_after_name_that_no_workflow_has_is_refused_when_rendering() {
    let rendered = render(
        &[crate::release_fixture::APP],
        Some("after = [\"Rust CI\", \"Linter\"]\n"),
    );
    assert!(
        rendered
            .err()
            .is_some_and(|error| error.to_string().contains("Linter"))
    );
}

#[test]
fn a_root_without_a_tracked_cargo_manifest_is_refused_when_rendering() {
    let rendered = render(
        &[crate::release_fixture::APP],
        Some("roots = [\".\", \"gone\"]\n"),
    );
    assert!(rendered.is_err());
}

#[test]
fn a_binary_whose_bin_target_does_not_exist_is_refused_when_rendering() {
    let rendered = render(
        &[crate::release_fixture::APP],
        Some("[[binaries]]\npackage = \"app\"\nbin = \"nope\"\n"),
    );
    assert!(
        rendered
            .err()
            .is_some_and(|error| error.to_string().contains("nope"))
    );
}

#[test]
fn a_workflow_name_with_filter_characters_is_escaped_in_the_trigger() -> TestResult {
    use velnor_gen::expected_files;
    use velnor_model::{RELEASE_WORKFLOW_PATH, RepoPath};

    let mut view = crate::release_fixture::release_view(
        &[crate::release_fixture::APP],
        Some("after = [\"Rust CI\", \"Build+Test\", \"checks[linux]\"]\n"),
    )?;
    for (file, name) in [("build", "Build+Test"), ("checks", "checks[linux]")] {
        view.files.insert(
            RepoPath::new(format!(".github/workflows/{file}.yml"))?,
            format!("name: {name}\non: push\n").into_bytes(),
        );
    }
    let text = expected_files(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    let tree = crate::yaml_tree::parse(&text)?;
    assert_eq!(
        tree.at(&["on", "workflow_run", "workflows"])
            .ok_or("no workflows")?
            .texts(),
        ["Rust CI", "Build\\+Test", "checks\\[linux\\]"]
    );
    Ok(())
}

#[test]
fn an_after_name_of_a_generator_owned_workflow_that_generation_retires_is_refused() -> TestResult {
    use velnor_gen::expected_files;
    use velnor_model::RepoPath;

    let mut view = crate::release_fixture::release_view(
        &[crate::release_fixture::APP],
        Some("after = [\"Rust CI\", \"Old Gen\"]\n"),
    )?;
    view.files.insert(
        RepoPath::new(".github/workflows/old.yml")?,
        velnor_render::seal(
            "# Generated by velnor-gen. Do not edit.\n",
            "name: Old Gen\non: push\n",
        )
        .into_bytes(),
    );
    let error = expected_files(&view).err().ok_or("accepted")?;
    assert!(error.to_string().contains("Old Gen"), "{error}");
    Ok(())
}

#[test]
fn the_tag_job_is_serialized_across_commits_and_both_jobs_with_the_app_token_refuse_commands()
-> TestResult {
    let (_, tree) = two_binaries()?;
    let tag = job(&tree, "tag")?;
    assert_eq!(
        tag.at(&["concurrency", "group"]).and_then(Node::text),
        Some("release-tag")
    );
    assert_eq!(
        tag.at(&["concurrency", "cancel-in-progress"])
            .and_then(Node::text),
        Some("false")
    );
    for id in ["release-pr", "tag"] {
        let all = steps(job(&tree, id)?);
        let refuse = all
            .iter()
            .position(|step| {
                step.get("run").and_then(Node::text).is_some_and(|run| {
                    run.starts_with(
                        "velnor-ci release check-config release-plz.toml tools/release-plz.toml ",
                    )
                })
            })
            .ok_or("no refuse step")?;
        let helper = all
            .iter()
            .position(|step| {
                step.get("uses")
                    .and_then(Node::text)
                    .is_some_and(|uses| uses.contains("/actions/setup-velnor@"))
            })
            .ok_or("no helper step")?;
        assert!(helper < refuse, "{id}");
        assert!(
            all.iter().all(|step| !step
                .get("run")
                .and_then(Node::text)
                .is_some_and(|run| run.contains("grep"))),
            "{id}"
        );
        let token = all
            .iter()
            .position(|step| {
                step.get("uses")
                    .and_then(Node::text)
                    .is_some_and(|uses| uses.starts_with("actions/create-github-app-token"))
            })
            .ok_or("no token step")?;
        assert!(refuse < token, "{id}");
    }
    Ok(())
}

#[test]
fn the_generated_release_workflow_cannot_be_an_after_workflow() -> TestResult {
    use velnor_gen::expected_files;
    use velnor_model::{RELEASE_WORKFLOW_PATH, RepoPath};

    let mut view = crate::release_fixture::release_view(
        &[crate::release_fixture::APP],
        Some("after = [\"Rust CI\", \"Release\"]\n"),
    )?;
    let _ = RepoPath::new("release.yml")?;
    view.files.insert(
        RELEASE_WORKFLOW_PATH,
        velnor_render::seal(
            "# Generated by velnor-gen. Do not edit.\n",
            "name: Release\non: push\n",
        )
        .into_bytes(),
    );
    let error = expected_files(&view).err().ok_or("accepted")?;
    assert!(error.to_string().contains("Release"), "{error}");
    Ok(())
}

#[test]
fn a_workflow_name_with_a_trailing_yaml_comment_or_quotes_is_the_plain_name() -> TestResult {
    use velnor_gen::expected_files;
    use velnor_model::RepoPath;

    let mut view = crate::release_fixture::release_view(
        &[crate::release_fixture::APP],
        Some("after = [\"Rust CI\", \"Qualification\", \"Checks\"]\n"),
    )?;
    for (file, text) in [
        ("q", "name: Qualification # required\non: push\n"),
        ("c", "name: \"Checks\" # quoted\non: push\n"),
    ] {
        view.files.insert(
            RepoPath::new(format!(".github/workflows/{file}.yml"))?,
            text.as_bytes().to_vec(),
        );
    }
    assert!(expected_files(&view).is_ok());
    Ok(())
}

#[test]
fn workflow_names_are_read_as_yaml_scalars() -> TestResult {
    use velnor_gen::expected_files;
    use velnor_model::RepoPath;

    let names = [
        "Rust CI",
        "Bob's CI",
        "Say \"hi\" \\ \u{e9}",
        "Checks # not a comment",
        "Tab\tName",
    ];
    let after: Vec<String> = names.iter().map(|name| format!("{name:?}")).collect();
    let mut view = crate::release_fixture::release_view(
        &[crate::release_fixture::APP],
        Some(&format!("after = [{}]\n", after.join(", "))),
    )?;
    for (file, text) in [
        ("a", "name: 'Bob''s CI' # note\non: push\n"),
        ("b", "name: \"Say \\\"hi\\\" \\\\ \\u00e9\"\non: push\n"),
        ("c", "name: 'Checks # not a comment'\non: push\n"),
        ("d", "name: \"Tab\\tName\"\non: push\n"),
    ] {
        view.files.insert(
            RepoPath::new(format!(".github/workflows/{file}.yml"))?,
            text.as_bytes().to_vec(),
        );
    }
    expected_files(&view)?;
    Ok(())
}

#[test]
fn an_after_list_that_misses_a_workflow_that_runs_on_push_is_refused() -> TestResult {
    use velnor_gen::{GenerateError, expected_files};
    use velnor_model::RepoPath;
    use velnor_release::ReleaseError;

    for (text, pushes) in [
        ("name: Extra\non: push\n", true),
        ("name: Extra\non: [pull_request, push]\n", true),
        (
            "name: Extra\n\"on\":\n  pull_request:\n  push:\n    branches: [main]\n",
            true,
        ),
        ("name: Extra\non:\n  - push\n", true),
        ("name: Extra\non: {push: {branches: [dev]}}\n", true),
        ("name: Extra\non: {pull_request: {}, push: null}\n", true),
        ("name: Extra\non: [push]\n", true),
        (
            "name: Extra\non: {pull_request: {branches: [push]}}\n",
            false,
        ),
        ("name: Extra\non: pull_request\n", false),
        ("name: Extra\non:\n  workflow_dispatch:\n", false),
    ] {
        let mut view = crate::release_fixture::release_view(
            &[crate::release_fixture::APP],
            Some("after = [\"Rust CI\"]\n"),
        )?;
        view.files.insert(
            RepoPath::new(".github/workflows/extra.yml")?,
            text.as_bytes().to_vec(),
        );
        match expected_files(&view) {
            Err(GenerateError::Release(ReleaseError::AfterMissingWorkflow { name })) => {
                assert!(pushes && name == "Extra", "{text}");
            }
            other => assert!(!pushes && other.is_ok(), "{text}: {other:?}"),
        }
    }
    Ok(())
}
