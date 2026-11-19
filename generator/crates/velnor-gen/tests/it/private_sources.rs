// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const SECTION: &str = "[private_sources]\napp_id = 'vars.DEPS_APP_ID'\nprivate_key = 'secrets.DEPS_APP_PRIVATE_KEY'\nrepositories = ['acme/shared-lib', 'acme/other-lib']\n";

fn private_repo(
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
        .insert(lock, old.replace("0.1.81", helper).into_bytes());
    Ok(repo)
}

fn jobs(text: &str) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
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
    assert!(compiling.iter().all(|index| *index > fetch), "{job}");
    for (index, step) in steps.iter().enumerate() {
        let rendered = step.to_string();
        assert_eq!(
            rendered.contains("private-token"),
            index == fetch || index == token,
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
            .filter(|id| id.starts_with("sources_"))
            .collect();
        let guards = [&steps[token], &steps[token + 1]]
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
    }
    assert!(checked > 0, "{text}");
    Ok(())
}
