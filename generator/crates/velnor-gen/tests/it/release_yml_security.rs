// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_render::ALLOWED_ACTIONS;

use crate::release_fixture::{
    APP, APP_TOKEN, Root, app, app_steps, jobs, render, steps, two_roots, uses,
};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;
#[test]
fn release_yml_has_empty_workflow_permissions_and_exact_job_permissions() -> TestResult {
    let (_, tree) = app()?;
    assert_eq!(tree.get("permissions"), Some(&Node::Map(Vec::new())));
    let permissions = |id: &str| -> Result<Vec<(String, String)>, Box<dyn Error>> {
        let node = tree
            .at(&["jobs", id, "permissions"])
            .ok_or("no permissions")?;
        node.keys()
            .into_iter()
            .map(|key| {
                let access = node.get(key).and_then(Node::text).ok_or("no access")?;
                Ok((key.to_owned(), access.to_owned()))
            })
            .collect()
    };
    let pair = |scope: &str, access: &str| (scope.to_owned(), access.to_owned());
    assert_eq!(
        permissions("gate")?,
        [
            pair("actions", "read"),
            pair("contents", "read"),
            pair("statuses", "read")
        ]
    );
    assert_eq!(permissions("release-pr")?, [pair("contents", "read")]);
    assert_eq!(permissions("tag")?, [pair("contents", "read")]);
    let (_, publishing) = render(
        &[Root {
            publishes: true,
            ..APP
        }],
        Some("[[binaries]]\npackage = \"app\"\n"),
    )?;
    let tag = publishing
        .at(&["jobs", "tag", "permissions"])
        .ok_or("no permissions")?;
    assert_eq!(tag.get("id-token").and_then(Node::text), Some("write"));
    assert!(
        publishing
            .at(&["jobs", "release-pr", "permissions"])
            .is_some_and(|p| p.get("id-token").is_none())
    );
    Ok(())
}

#[test]
fn release_yml_app_token_permissions_are_exact_and_scoped_to_this_repository() -> TestResult {
    let (_, tree) = two_roots()?;
    let expected = [
        (
            "release-pr",
            ["permission-contents", "permission-pull-requests"],
            ["write", "write"],
        ),
        (
            "tag",
            ["permission-contents", "permission-pull-requests"],
            ["write", "read"],
        ),
    ];
    for (id, keys, access) in expected {
        let job = tree.at(&["jobs", id]).ok_or("no job")?;
        let tokens = app_steps(job);
        assert_eq!(tokens.len(), 1, "{id}");
        let with = tokens[0].get("with").ok_or("no with")?;
        let mut want: Vec<&str> = vec!["client-id", "private-key", "owner", "repositories"];
        want.extend(keys);
        assert_eq!(with.keys(), want, "{id}");
        for (key, level) in keys.iter().zip(access) {
            assert_eq!(
                with.get(key).and_then(Node::text),
                Some(level),
                "{id} {key}"
            );
        }
        assert_eq!(
            with.get("repositories").and_then(Node::text),
            Some("${{ github.event.repository.name }}")
        );
        assert_eq!(
            with.get("owner").and_then(Node::text),
            Some("${{ github.repository_owner }}")
        );
    }
    Ok(())
}

#[test]
fn release_yml_jobs_that_hold_the_app_key_use_the_release_environment() -> TestResult {
    let (_, tree) = two_roots()?;
    for (id, job) in jobs(&tree)? {
        let holds_key = !app_steps(job).is_empty();
        let environment = job.get("environment").and_then(Node::text);
        assert_eq!(environment == Some("release"), holds_key, "{id}");
        assert_eq!(environment.is_some(), holds_key, "{id}");
    }
    Ok(())
}

#[test]
fn release_yml_checks_out_without_credentials_caches_nothing_and_pins_every_action() -> TestResult {
    let (text, tree) = two_roots()?;
    for (id, job) in jobs(&tree)? {
        for step in steps(job) {
            let Some(action) = uses(step) else { continue };
            let (name, reference) = action.split_once('@').ok_or("unpinned")?;
            assert!(ALLOWED_ACTIONS.contains(&name), "{id} {name}");
            assert!(
                reference.len() == 40 && reference.bytes().all(|b| b.is_ascii_hexdigit()),
                "{id} {action}"
            );
            let with = |key: &str| step.at(&["with", key]).and_then(Node::text);
            if name == "actions/checkout" {
                assert_eq!(with("persist-credentials"), Some("false"), "{id}");
            }
            if name == "jdx/mise-action" {
                assert_eq!(with("cache"), Some("false"), "{id}");
            }
            assert!(!name.starts_with("actions/cache"), "{id}");
        }
    }
    assert!(!text.contains("actions/cache"));
    Ok(())
}

#[test]
fn release_yml_hands_the_app_token_only_to_release_plz_after_everything_is_installed() -> TestResult
{
    let (_, tree) = two_roots()?;
    for id in ["release-pr", "tag"] {
        let all = steps(tree.at(&["jobs", id]).ok_or("no job")?);
        let created = all
            .iter()
            .position(|step| uses(step).is_some_and(|u| u.starts_with(APP_TOKEN)))
            .ok_or("no token step")?;
        for step in &all[..created] {
            let env_is = |expected: &[(&str, &str)]| {
                step.get("env").is_some_and(|env| {
                    let mut keys = env.keys();
                    keys.sort_unstable();
                    keys == expected.iter().map(|(key, _)| *key).collect::<Vec<_>>()
                        && expected
                            .iter()
                            .all(|(key, value)| env.get(key).and_then(Node::text) == Some(*value))
                })
            };
            let newest = step.get("id").and_then(Node::text) == Some("newest")
                && env_is(&[
                    ("BRANCH", "${{ github.event.repository.default_branch }}"),
                    ("COMMIT", "${{ github.event.workflow_run.head_sha }}"),
                    ("GH_TOKEN", "${{ github.token }}"),
                    ("REPOSITORY", "${{ github.repository }}"),
                ]);
            let branch_only =
                env_is(&[("BRANCH", "${{ github.event.repository.default_branch }}")]);
            let check = step
                .get("run")
                .and_then(Node::text)
                .is_some_and(|run| run.starts_with("velnor-ci release check-config"));
            assert!(
                newest || branch_only || check || step.get("env").is_none(),
                "{id}"
            );
        }
        for step in &all[created + 1..] {
            let token = step.at(&["env", "GITHUB_TOKEN"]).and_then(Node::text);
            let runs_release_plz = step
                .get("run")
                .and_then(Node::text)
                .is_some_and(|run| run.starts_with("\"$RUNNER_TEMP/release-plz\" release"));
            assert_eq!(
                token == Some("${{ steps.app.outputs.token }}"),
                runs_release_plz,
                "{id}"
            );
            assert!(uses(step).is_none(), "{id}");
        }
    }
    Ok(())
}

#[test]
fn release_yml_runs_one_release_plz_command_per_root_and_records_every_output() -> TestResult {
    let (_, tree) = two_roots()?;
    let runs = |id: &str| -> Result<Vec<&str>, Box<dyn Error>> {
        Ok(steps(tree.at(&["jobs", id]).ok_or("no job")?)
            .iter()
            .filter_map(|step| step.get("run").and_then(Node::text))
            .filter(|run| !run.starts_with("gh ") && !run.starts_with("git "))
            .collect())
    };
    let base = "--forge github --git-token \"$GITHUB_TOKEN\"";
    let exec = "\"$RUNNER_TEMP/release-plz\"";
    let refuse = "velnor-ci release check-config release-plz.toml tools/release-plz.toml --tag-pattern release-plz.toml 'app-v{{ version }}' --tag-pattern release-plz.toml '{{ package }}-v{{ version }}' --tag-pattern tools/release-plz.toml 'tools-v{{ version }}' --tag-pattern tools/release-plz.toml '{{ package }}-v{{ version }}' --repo \"$REPOSITORY\" --manifests";
    let mut prepared: Vec<String> = [
        "curl --fail --silent --show-error --location --output \"$RUNNER_TEMP/release-plz.tar.gz\" 'https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3/release-plz-x86_64-unknown-linux-gnu.tar.gz'",
        "printf '%s  %s/release-plz.tar.gz\\n' a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90 \"$RUNNER_TEMP\" > \"$RUNNER_TEMP/release-plz.sha256\"",
        "sha256sum --check \"$RUNNER_TEMP/release-plz.sha256\"",
        "tar --extract --gzip --file \"$RUNNER_TEMP/release-plz.tar.gz\" --directory \"$RUNNER_TEMP\" release-plz",
        "rustup toolchain install stable --profile minimal --no-self-update",
        refuse,
    ]
    .map(str::to_owned)
    .to_vec();
    let mut pr_runs = prepared.clone();
    pr_runs.extend([
        format!("{exec} release-pr {base} --manifest-path Cargo.toml --config release-plz.toml"),
        format!(
            "{exec} release-pr {base} --manifest-path tools/Cargo.toml --config tools/release-plz.toml"
        ),
    ]);
    assert_eq!(runs("release-pr")?, pr_runs);
    prepared.extend([
            format!("{exec} release {base} --manifest-path Cargo.toml --config release-plz.toml -o json > \"$RUNNER_TEMP/release-0.json\""),
            format!("{exec} release {base} --manifest-path tools/Cargo.toml --config tools/release-plz.toml -o json > \"$RUNNER_TEMP/release-1.json\""),
            "velnor-ci release record \"$RUNNER_TEMP/release-0.json\" \"$RUNNER_TEMP/release-1.json\"".to_owned(),
    ]);
    assert_eq!(runs("tag")?, prepared);
    for id in ["gate", "release-pr", "tag"] {
        for run in runs(id)? {
            assert!(
                !run.contains(['\n', ';', '|', '`']) && !run.contains("&&") && !run.contains("${{"),
                "{run}"
            );
        }
    }
    Ok(())
}

#[test]
fn release_yml_tag_rechecks_the_live_tip_before_the_app_token_and_skips_when_it_moved() -> TestResult
{
    let (_, tree) = two_roots()?;
    let all = steps(tree.at(&["jobs", "tag"]).ok_or("no job")?);
    let newest = all
        .iter()
        .position(|step| step.get("id").and_then(Node::text) == Some("newest"))
        .ok_or("no newest step")?;
    let created = all
        .iter()
        .position(|step| uses(step).is_some_and(|u| u.starts_with(APP_TOKEN)))
        .ok_or("no token step")?;
    assert!(newest < created);
    assert!(all[newest].get("if").is_none());
    for step in &all[newest + 1..] {
        assert_eq!(
            step.get("if").and_then(Node::text),
            Some("${{ steps.newest.outputs.newest == 'true' }}")
        );
    }
    Ok(())
}
