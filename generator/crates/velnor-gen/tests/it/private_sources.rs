// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

pub(crate) const SECTION: &str = "[private_sources]\napp_id = 'vars.DEPS_APP_ID'\nprivate_key = 'secrets.DEPS_APP_PRIVATE_KEY'\nrepositories = ['acme/shared-lib', 'acme/other-lib']\n";

pub(crate) fn private_repo(
    roots: &[&str],
    section: &str,
    helper: &str,
) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(roots, None)?;
    repo.files
        .insert(path(".velnor/rust-ci.toml")?, section.as_bytes().to_vec());
    let lock = path(".velnor/rust-ci.lock")?;
    let old = String::from_utf8(repo.files[&lock].clone())?;
    repo.files
        .insert(lock, old.replace("0.1.92", helper).into_bytes());
    Ok(repo)
}

pub(crate) fn jobs(text: &str) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let yaml: serde_json::Value = crate::yaml_tree::document(text)?;
    Ok(yaml["jobs"]
        .as_object()
        .ok_or("jobs missing")?
        .values()
        .cloned()
        .collect())
}

fn cargo_commands(steps: &[serde_json::Value]) -> Vec<usize> {
    steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            step["run"].as_str().is_some_and(|run| {
                run.contains("\"$VELNOR_EXEC\" run -- cargo")
                    && !run.contains("cargo-fmt")
                    && !run.contains("cargo-deny")
                    && !run.contains("cargo-machete")
            })
        })
        .map(|(index, _)| index)
        .collect()
}

fn assert_phase(
    job: &serde_json::Value,
    steps: &[serde_json::Value],
    token: usize,
    fetch: usize,
) -> TestResult {
    let compiling = cargo_commands(steps);
    assert_eq!(token + 1, fetch);
    let check = token.checked_sub(1).ok_or("no step before the token")?;
    assert!(compiling.iter().all(|index| *index > fetch), "{job}");
    for (index, step) in steps.iter().enumerate() {
        let rendered = step.to_string();
        assert_eq!(
            rendered.contains("private-token"),
            index == fetch || index == token || index == check,
            "{rendered}"
        );
        assert_eq!(
            rendered.contains("DEPS_APP_PRIVATE_KEY"),
            index == token,
            "{rendered}"
        );
    }
    for index in compiling {
        let step = &steps[index];
        assert!(
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains(" --offline")),
            "{step}"
        );
        assert_eq!(step["env"]["CARGO_NET_OFFLINE"], "true", "{step}");
    }
    let fetch_step = &steps[fetch];
    assert_eq!(
        fetch_step["env"]["VELNOR_PRIVATE_TOKEN"],
        "${{ steps.private-token.outputs.token }}"
    );
    assert_eq!(fetch_step["env"]["VELNOR_API_URL"], "${{ github.api_url }}");
    let run = fetch_step["run"].as_str().ok_or("fetch run missing")?;
    assert!(
        run.contains("--manifest-path ") && run.contains("Cargo.toml"),
        "{run}"
    );
    let create = &steps[token];
    assert_eq!(create["with"]["permission-contents"], "read");
    assert!(
        create["with"].get("skip-token-revoke").is_none(),
        "the action's post-job revocation must stay on as the backstop for a cancelled fetch: {create}"
    );
    assert_eq!(create["with"]["owner"], "acme");
    assert_eq!(
        create["with"]["repositories"].as_str().map(str::trim_end),
        Some("shared-lib\nother-lib")
    );
    assert!(
        steps.iter().enumerate().all(|(index, step)| {
            !step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("jdx/mise-action@"))
                || index < token
        }),
        "{job}"
    );
    assert!(
        create["if"]
            .as_str()
            .is_some_and(|guard| guard.contains("head.repo.full_name == github.repository"))
    );
    Ok(())
}

#[test]
fn the_token_reaches_only_the_fetch_step_and_every_cargo_command_after_it_runs_offline()
-> TestResult {
    for shape in ["", "shape = 'groups'\n"] {
        let repo = private_repo(
            &["Cargo.toml", "tools/Cargo.toml"],
            &format!("{shape}{SECTION}"),
            "0.1.86",
        )?;
        let text = workflow(&repo)?;
        let mut fetches = 0;
        for job in jobs(&text)? {
            let steps = job["steps"].as_array().ok_or("steps missing")?;
            let token = steps.iter().position(|s| s["id"] == "private-token");
            let fetch = steps.iter().position(|s| {
                s["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("\"$VELNOR_EXEC\" fetch "))
            });
            assert_eq!(token.is_some(), fetch.is_some(), "{job}");
            let (Some(token), Some(fetch)) = (token, fetch) else {
                continue;
            };
            fetches += 1;
            assert_phase(&job, steps, token, fetch)?;
        }
        assert!(fetches > 0, "{text}");
        assert!(
            !text.contains("secrets.") || text.matches("DEPS_APP_PRIVATE_KEY").count() == fetches
        );
    }
    Ok(())
}

#[test]
fn without_the_section_the_workflow_has_no_token_and_no_offline_flag() -> TestResult {
    let mut repo = private_repo(&["Cargo.toml"], SECTION, "0.1.86")?;
    repo.files.insert(
        path(".velnor/rust-ci.toml")?,
        b"full_run = 'off'\n".to_vec(),
    );
    let text = workflow(&repo)?;
    for forbidden in [
        "create-github-app-token",
        "VELNOR_PRIVATE_TOKEN",
        "--offline",
        "CARGO_NET_OFFLINE",
        "\"$VELNOR_EXEC\" fetch",
    ] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
    Ok(())
}

#[test]
fn a_misconfigured_section_or_an_old_helper_is_refused() -> TestResult {
    let bad = [
        SECTION.replace("vars.DEPS_APP_ID", "secrets.DEPS_APP_ID"),
        SECTION.replace("secrets.DEPS_APP_PRIVATE_KEY", "vars.DEPS_APP_PRIVATE_KEY"),
        SECTION.replace("secrets.DEPS_APP_PRIVATE_KEY", "secrets.X }} ${{ secrets.Y"),
        SECTION.replace("'acme/other-lib'", "'other/lib'"),
        SECTION.replace("'acme/other-lib'", "'acme/shared-lib'"),
        SECTION.replace("'acme/shared-lib', 'acme/other-lib'", ""),
        SECTION.replace("'acme/shared-lib'", "'acme'"),
        format!("{SECTION}extra = 1\n"),
        SECTION.replace("vars.DEPS_APP_ID", "vars.GITHUB_DEPS_APP_ID"),
        SECTION.replace("secrets.DEPS_APP_PRIVATE_KEY", "secrets.github_deps_key"),
    ];
    for section in bad {
        let repo = private_repo(&["Cargo.toml"], &section, "0.1.86")?;
        assert!(workflow(&repo).is_err(), "{section}");
    }
    for released in ["0.1.81", "0.1.82", "0.1.83", "0.1.84", "0.1.85"] {
        let old = private_repo(&["Cargo.toml"], SECTION, released)?;
        assert!(
            workflow(&old)
                .is_err_and(|error| error.to_string().contains("pin velnor-ci 0.1.86 or later")),
            "{released}"
        );
    }
    let current = private_repo(&["Cargo.toml"], SECTION, "0.1.86")?;
    assert!(workflow(&current).is_ok());
    Ok(())
}

fn misses(condition: &str, hits: &[&str]) -> Result<bool, Box<dyn std::error::Error>> {
    let start = condition
        .find("(steps.sources_")
        .ok_or("no sources guard")?;
    let group = &condition[start + 1..];
    let group = &group[..group.find(')').ok_or("unclosed guard")?];
    Ok(group.split(" || ").any(|term| {
        let id = term
            .trim_start_matches("steps.")
            .trim_end_matches(".outputs.cache-hit != 'true'");
        !hits.contains(&id)
    }))
}

#[test]
fn an_exact_sources_hit_skips_the_token_and_the_fetch_and_a_partial_hit_fetches() -> TestResult {
    let repo = private_repo(&["Cargo.toml", "tools/Cargo.toml"], SECTION, "0.1.86")?;
    let text = workflow(&repo)?;
    let mut checked = 0;
    for job in jobs(&text)? {
        let steps = job["steps"].as_array().ok_or("steps missing")?;
        let Some(token) = steps.iter().position(|s| s["id"] == "private-token") else {
            continue;
        };
        let restores: Vec<&str> = steps[..token]
            .iter()
            .filter_map(|s| s["id"].as_str())
            .filter(|id| id.starts_with("sources_") && *id != "sources_locks")
            .collect();
        let guards = [&steps[token], &steps[token + 1], &steps[token - 1]]
            .map(|step| step["if"].as_str().unwrap_or_default().to_owned());
        if restores.is_empty() {
            assert!(guards.iter().all(|g| !g.contains("cache-hit")), "{job}");
            continue;
        }
        checked += 1;
        for guard in &guards {
            assert!(!misses(guard, &restores)?, "{guard}");
            for skipped in &restores {
                let partial: Vec<&str> = restores
                    .iter()
                    .copied()
                    .filter(|id| id != skipped)
                    .collect();
                assert!(misses(guard, &partial)?, "{guard}");
            }
            assert!(misses(guard, &[])?, "{guard}");
        }
        assert!(guards[0].contains("head.repo.full_name == github.repository"));
        assert_eq!(steps[token - 1]["id"], "private-check", "{job}");
    }
    assert!(checked > 0, "{text}");
    Ok(())
}

#[test]
fn the_dependency_policy_job_fetches_before_cargo_deny_resolves_the_graph() -> TestResult {
    let mut repo = private_repo(&["Cargo.toml", "tools/Cargo.toml"], SECTION, "0.1.86")?;
    for (name, text) in [("deny.toml", ""), ("mise.toml", crate::events::DENY_MISE)] {
        let name = path(name)?;
        repo.tracked_paths.insert(name.clone());
        repo.files.insert(name, text.as_bytes().to_vec());
    }
    let text = workflow(&repo)?;
    let mut checked = 0;
    for job in jobs(&text)? {
        let steps = job["steps"].as_array().ok_or("steps missing")?;
        let deny: Vec<usize> = steps
            .iter()
            .enumerate()
            .filter(|(_, step)| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("cargo-deny"))
            })
            .map(|(index, _)| index)
            .collect();
        let Some(first) = deny.first() else { continue };
        checked += 1;
        let token = steps.iter().position(|s| s["id"] == "private-token");
        assert!(token.is_some_and(|token| token + 1 < *first), "{job}");
        let fetch = steps[token.unwrap_or_default() + 1]["run"]
            .as_str()
            .unwrap_or_default();
        assert!(
            fetch.contains("Cargo.toml") && fetch.contains("tools/Cargo.toml"),
            "{fetch}"
        );
        for index in deny {
            assert_eq!(steps[index]["env"]["CARGO_NET_OFFLINE"], "true", "{job}");
        }
    }
    assert_eq!(checked, 1, "{text}");
    Ok(())
}

#[test]
fn a_merge_queue_entry_gets_the_token_only_when_the_check_before_it_allows_it() -> TestResult {
    let repo = private_repo(&["Cargo.toml", "tools/Cargo.toml"], SECTION, "0.1.86")?;
    let text = workflow(&repo)?;
    let mut checked = 0;
    for job in jobs(&text)? {
        let steps = job["steps"].as_array().ok_or("steps missing")?;
        let Some(token) = steps.iter().position(|s| s["id"] == "private-token") else {
            continue;
        };
        checked += 1;
        let check = steps
            .iter()
            .position(|s| s["id"] == "private-check")
            .ok_or("no check step")?;
        assert_eq!(check + 1, token, "{job}");
        let check = &steps[check];
        let run = check["run"].as_str().ok_or("check run missing")?;
        assert!(run.ends_with("private-token-allowed"), "{run}");
        assert_eq!(
            check["if"].as_str().map(|g| g.contains("'merge_group'")),
            Some(true)
        );
        assert_eq!(
            check["env"]["VELNOR_MERGE_GROUP_HEAD_REF"],
            "${{ github.event.merge_group.head_ref }}"
        );
        assert_eq!(check["env"]["GITHUB_TOKEN"], "${{ github.token }}");
        assert_eq!(check["env"]["VELNOR_API_URL"], "${{ github.api_url }}");
        assert!(
            !check.to_string().contains("DEPS_APP_PRIVATE_KEY"),
            "{check}"
        );
        let guard = steps[token]["if"].as_str().ok_or("token guard missing")?;
        assert!(
            guard.contains("github.event_name == 'merge_group' && steps.private-check.outputs.allowed == 'true'"),
            "{guard}"
        );
        assert!(
            guard.contains("github.event_name != 'merge_group' && (github.event_name != 'pull_request' || (github.event.pull_request.head.repo.full_name == github.repository && !endsWith(github.actor, '[bot]') && !endsWith(github.event.pull_request.user.login, '[bot]')))"),
            "{guard}"
        );
        assert_eq!(job["permissions"]["pull-requests"], "read", "{job}");
    }
    assert!(checked > 0, "{text}");
    Ok(())
}

#[test]
fn the_token_action_gets_the_configured_value_as_its_client_id() -> TestResult {
    let text = workflow(&private_repo(&["Cargo.toml"], SECTION, "0.1.86")?)?;
    let mut checked = 0;
    for job in jobs(&text)? {
        for step in job["steps"].as_array().into_iter().flatten() {
            if step["id"] == "private-token" {
                checked += 1;
                assert_eq!(step["with"]["client-id"], "${{ vars.DEPS_APP_ID }}");
                assert!(step["with"].get("app-id").is_none(), "{step}");
            }
        }
    }
    assert!(checked > 0, "{text}");
    Ok(())
}

#[test]
fn test_binaries_built_with_private_sources_are_never_uploaded_as_an_artifact() -> TestResult {
    let config = format!("shape = 'groups'\n{SECTION}");
    let text = workflow(&private_repo(&["Cargo.toml"], &config, "0.1.86")?)?;
    for job in jobs(&text)? {
        for step in job["steps"].as_array().into_iter().flatten() {
            let name = step["with"]["name"].as_str().unwrap_or_default();
            assert!(!name.starts_with("nextest-archive"), "{step}");
            let run = step["run"].as_str().unwrap_or_default();
            assert!(!run.contains("nextest archive"), "{step}");
        }
    }
    assert!(text.contains("private-token"), "{text}");
    Ok(())
}

#[test]
fn a_cargo_config_include_is_refused_so_no_included_source_can_hide_the_private_dependencies()
-> TestResult {
    for config in ["", "[build]\njobs = 2\n"] {
        let mut repo = private_repo(&["Cargo.toml"], SECTION, "0.1.86")?;
        repo.files.insert(
            path(".cargo/config.toml")?,
            format!("include = [\"private.toml\"]\n{config}").into_bytes(),
        );
        let message = workflow(&repo).err().map(|e| e.to_string());
        assert!(
            message.as_deref().is_some_and(|m| m.contains("include")),
            "{message:?}"
        );
    }
    Ok(())
}
