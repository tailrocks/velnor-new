// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use velnor_actions_contract::{HOSTED_SUFFIX, REQUIRED_JOB_ID, SCALE_SUFFIX, StepKind};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};

use super::verification_tools_fixture as fixture;

use super::waste_oracle as oracle;

const HOOK_CLOSURE_ACTION: &str = ".github/actions/task-hook-closure/action.yml";

#[test]
fn verification_task_jobs_render_auto_install_off() -> Result<(), Box<dyn Error>> {
    let repo = fixture::repo(fixture::REGEX, fixture::LOCK)?;
    let prepared = prepare(repo.path())?;
    let files = oracle::files_of(&render_staged_tree(&prepared)?)?;
    let action = oracle::file_tokens(&files, HOOK_CLOSURE_ACTION)?;
    for token in ["install", "--locked"] {
        assert!(
            action.contains(token),
            "{HOOK_CLOSURE_ACTION} lacks {token}"
        );
    }
    let jobs = oracle::scan(&files, |_| true)?;
    for suffix in [HOSTED_SUFFIX, SCALE_SUFFIX] {
        let id = format!("task-hook-closure{suffix}");
        assert!(
            oracle::find(&jobs, &id)?.is_user(),
            "{id} is not a toolchain job"
        );
    }
    let found = oracle::violations(&jobs);
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

#[test]
fn verification_task_preserves_seven_tool_hook_closure_and_required_fan_in()
-> Result<(), Box<dyn Error>> {
    let repo = fixture::repo(fixture::REGEX, fixture::LOCK)?;
    let prepared = prepare(repo.path())
        .map_err(|error| format!("seven-tool VerificationTask prepare rejected: {error}"))?;
    let jobs = finalized_jobs(&prepared)
        .map_err(|error| format!("seven-tool VerificationTask renderer rejected: {error}"))?;
    let required = jobs.get(REQUIRED_JOB_ID).ok_or("missing Required")?;
    let tasks: Vec<_> = jobs
        .iter()
        .filter(|(id, _)| id.starts_with("task-hook-closure"))
        .collect();
    assert_eq!(tasks.len(), 2, "missing hosted/scale hook closure variants");
    for suffix in [HOSTED_SUFFIX, SCALE_SUFFIX] {
        assert!(jobs.contains_key(&format!("task-hook-closure{suffix}")));
    }
    for (id, job) in tasks {
        assert!(required.needs.contains(id), "Required omitted {id}");
        let scripts: Vec<_> = job
            .steps
            .iter()
            .filter_map(|step| match &step.kind {
                StepKind::Shell { run, .. } => run.last().map(String::as_str),
                _ => None,
            })
            .collect();
        let config = emitted_file(&scripts, "mise.toml")?;
        let lock = emitted_file(&scripts, "mise.lock")?;
        assert_eq!(lock["lockfile_version"].as_integer(), Some(4));
        let tools = config["tools"].as_table().ok_or("missing selected tools")?;
        assert_eq!(tools.len(), fixture::TOOLS.len());
        for (key, version) in fixture::TOOLS {
            assert_eq!(tools[key]["version"].as_str(), Some(version), "{key}");
        }
        assert_eq!(
            tools[fixture::KINGFISHER]["matching_regex"].as_str(),
            Some(fixture::REGEX)
        );
        assert_eq!(
            lock["tools"],
            projected_tools(fixture::LOCK, "linux-x64")?,
            "selected lock provenance changed"
        );
        assert_eq!(
            lock["tools"]["hk"][0]["backend"].as_str(),
            Some("packslip:github.com/jdx/hk")
        );
        assert_eq!(
            lock["tools"]["hk"][0]["platforms.linux-x64"]["signer"].as_str(),
            Some("sigstore-oidc:https://github.com/jdx/hk/.github/workflows/release.yml")
        );
        let script = scripts.join("\n");
        for bytes in [
            fixture::source(fixture::REGEX).into_bytes(),
            fixture::LOCK.as_bytes().to_vec(),
            fixture::RUST.as_bytes().to_vec(),
        ] {
            let mut digest = String::with_capacity(64);
            for byte in Sha256::digest(bytes) {
                write!(&mut digest, "{byte:02x}")?;
            }
            assert!(
                script.contains(&digest),
                "source digest not bound: {digest}"
            );
        }
        assert!(script.contains("--skip-tools"));
    }
    assert!(!render_staged_tree(&prepared)?.is_empty());
    Ok(())
}

#[test]
fn build_task_mise_v4_artifact_metadata_survives_selection_and_rendering()
-> Result<(), Box<dyn Error>> {
    const NEXT: &str = "aqua:nextest-rs/nextest/cargo-nextest";
    for provenance_verified in [false, true] {
        let lock_input = fixture::lock_with_artifact_metadata(
            NEXT,
            "macos-arm64",
            provenance_verified,
            "nextest-rs",
        )?;
        let repo = fixture::build_task_repo_with_lock(&lock_input)?;
        let prepared = prepare(repo.path())
            .map_err(|error| format!("v4 BuildTask prepare rejected: {error}"))?;
        let jobs = finalized_jobs(&prepared)?;
        let job = jobs
            .get("task-mise-v4-tools")
            .ok_or("BuildTask job is missing")?;
        let scripts = job
            .steps
            .iter()
            .filter_map(|step| match &step.kind {
                StepKind::Shell { run, .. } => run.last().map(String::as_str),
                _ => None,
            })
            .collect::<Vec<_>>();
        let emitted_lock = emitted_file(&scripts, "mise.lock")?;
        assert_eq!(emitted_lock["lockfile_version"].as_integer(), Some(4));
        let selected_platform = "platforms.macos-arm64";
        let mut expected: toml::Value = toml::from_str(&lock_input)?;
        let expected_tools = expected["tools"]
            .as_table_mut()
            .ok_or("missing fixture tools")?;
        expected_tools.retain(|key, _| key == NEXT || key == "mr-boxington" || key == "rust");
        for (_, entries) in expected_tools.iter_mut() {
            for entry in entries.as_array_mut().ok_or("missing tool entries")? {
                let entry = entry.as_table_mut().ok_or("missing fixture tool")?;
                entry.remove("specifiers");
                let other_platforms = entry
                    .keys()
                    .filter(|key| key.starts_with("platforms.") && *key != selected_platform)
                    .cloned()
                    .collect::<Vec<_>>();
                for platform in other_platforms {
                    entry.remove(&platform);
                }
            }
        }
        assert_eq!(emitted_lock["tools"], expected["tools"]);
        assert_eq!(
            emitted_lock["tools"][NEXT][0][selected_platform]["provenance_verified"].as_bool(),
            Some(provenance_verified)
        );
        assert_eq!(
            emitted_lock["tools"][NEXT][0][selected_platform]["repository_ids"]["owner"].as_str(),
            Some("nextest-rs")
        );
        assert_eq!(
            emitted_lock["tools"][NEXT][0][selected_platform]["checksum_source"].as_str(),
            Some("mise-versions")
        );
        assert!(!render_staged_tree(&prepared)?.is_empty());
    }
    Ok(())
}

#[test]
fn verification_task_preserves_optional_v4_artifact_metadata() -> Result<(), Box<dyn Error>> {
    for provenance_verified in [false, true] {
        let lock = fixture::lock_with_artifact_metadata(
            fixture::KINGFISHER,
            "linux-x64",
            provenance_verified,
            "mongodb",
        )?;
        let repo = fixture::repo(fixture::REGEX, &lock)?;
        let prepared = prepare(repo.path())?;
        let jobs = finalized_jobs(&prepared)?;
        let expected = projected_tools(&lock, "linux-x64")?;
        for suffix in [HOSTED_SUFFIX, SCALE_SUFFIX] {
            let id = format!("task-hook-closure{suffix}");
            let job = jobs.get(&id).ok_or("VerificationTask job is missing")?;
            let scripts = job
                .steps
                .iter()
                .filter_map(|step| match &step.kind {
                    StepKind::Shell { run, .. } => run.last().map(String::as_str),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let emitted = emitted_file(&scripts, "mise.lock")?;
            assert_eq!(emitted["tools"], expected, "{id}");
            assert_eq!(
                emitted["tools"][fixture::KINGFISHER][0]["platforms.linux-x64"]
                    ["provenance_verified"]
                    .as_bool(),
                Some(provenance_verified),
                "{id}"
            );
            assert_eq!(
                emitted["tools"][fixture::KINGFISHER][0]["platforms.linux-x64"]
                    ["repository_ids"]["owner"]
                    .as_str(),
                Some("mongodb"),
                "{id}"
            );
        }
    }
    Ok(())
}

#[test]
fn verification_task_rejects_mismatched_github_selector_lock() -> Result<(), Box<dyn Error>> {
    let repo = fixture::repo("^different[.]tgz$", fixture::LOCK)?;
    let error = prepare(repo.path())
        .err()
        .ok_or("mismatched selector was accepted")?;
    assert!(
        error
            .to_string()
            .contains("verification_tool_identity_or_lock"),
        "{error}"
    );
    Ok(())
}

#[test]
fn verification_task_rejects_template_selectors_even_with_matching_lock()
-> Result<(), Box<dyn Error>> {
    for regex in [
        "^[{{ env.HOME }}]+$",
        "^[{% if true %}asset{% endif %}]+$",
        "^[{# literal comment #}asset]+$",
    ] {
        let lock = fixture::lock_with_regex(regex)?;
        let repo = fixture::repo(regex, &lock)?;
        let error = prepare(repo.path())
            .err()
            .ok_or("template selector was accepted")?;
        assert!(
            error.to_string().contains("verification_tool_selector"),
            "{error}"
        );
    }
    Ok(())
}

#[test]
fn verification_task_rejects_noncanonical_hk_backend() -> Result<(), Box<dyn Error>> {
    let lock = fixture::LOCK.replace("packslip:github.com/jdx/hk", "github:jdx/hk");
    let repo = fixture::repo(fixture::REGEX, &lock)?;
    let error = prepare(repo.path())
        .err()
        .ok_or("foreign HK backend was accepted")?;
    assert!(
        error
            .to_string()
            .contains("verification_tool_backend_unsupported"),
        "{error}"
    );
    Ok(())
}

#[test]
fn verification_task_rejects_unknown_selector_option() -> Result<(), Box<dyn Error>> {
    let repo = fixture::repo(fixture::REGEX, fixture::LOCK)?;
    let path = repo.path().join(".velnor/hk-check/mise.toml");
    let mut source: toml::Value = toml::from_str(&std::fs::read_to_string(&path)?)?;
    source["tools"][fixture::KINGFISHER]
        .as_table_mut()
        .ok_or("missing source selector")?
        .insert(
            "unexpected_option".to_owned(),
            toml::Value::String("literal".to_owned()),
        );
    std::fs::write(path, toml::to_string(&source)?)?;
    let mut lock: toml::Value = toml::from_str(fixture::LOCK)?;
    lock["tools"][fixture::KINGFISHER][0]["options"]
        .as_table_mut()
        .ok_or("missing lock selector")?
        .insert(
            "unexpected_option".to_owned(),
            toml::Value::String("literal".to_owned()),
        );
    std::fs::write(
        repo.path().join(".velnor/hk-check/mise.lock"),
        toml::to_string(&lock)?,
    )?;
    let error = prepare(repo.path())
        .err()
        .ok_or("unknown selector option was accepted")?;
    assert!(
        error.to_string().contains("verification_tool_selector"),
        "{error}"
    );
    Ok(())
}

fn emitted_file(scripts: &[&str], name: &str) -> Result<toml::Value, Box<dyn Error>> {
    let suffix = format!(" > \"$task_root/{name}\"");
    let statements: Vec<_> = scripts
        .iter()
        .flat_map(|script| script.split("; "))
        .filter_map(|statement| statement.strip_suffix(&suffix))
        .filter(|statement| statement.starts_with("printf '%s\\n' "))
        .collect();
    assert_eq!(
        statements.len(),
        1,
        "missing or duplicate private {name} writer"
    );
    let payload = statements[0]
        .strip_prefix("printf '%s\\n' ")
        .ok_or("unexpected private writer")?;
    assert!(payload.len() < 64 * 1024);
    let mut input = payload.chars().peekable();
    let mut lines = Vec::new();
    while let Some(quote) = input.next() {
        assert!(
            matches!(quote, '\'' | '"'),
            "unquoted private writer argument"
        );
        let mut line = String::new();
        loop {
            let character = input.next().ok_or("unterminated private writer argument")?;
            if character == quote {
                break;
            }
            if quote == '"' && character == '\\' {
                let escaped = input.next().ok_or("unterminated private writer escape")?;
                assert!(matches!(escaped, '\\' | '"' | '$' | '`'));
                line.push(escaped);
            } else {
                assert!(quote == '\'' || !matches!(character, '$' | '`'));
                line.push(character);
            }
        }
        lines.push(line);
        if input.peek().is_some() {
            assert_eq!(input.next(), Some(' '));
        }
    }
    Ok(toml::from_str(&lines.join("\n"))?)
}

fn projected_tools(lock: &str, platform: &str) -> Result<toml::Value, Box<dyn Error>> {
    let mut expected: toml::Value = toml::from_str(lock)?;
    for (_, entries) in expected["tools"]
        .as_table_mut()
        .ok_or("missing fixture tools")?
        .iter_mut()
    {
        for entry in entries.as_array_mut().ok_or("missing tool entries")? {
            let entry = entry.as_table_mut().ok_or("missing fixture tool")?;
            entry.remove("specifiers");
            let selected_platform = format!("platforms.{platform}");
            let other_platforms = entry
                .keys()
                .filter(|key| key.starts_with("platforms.") && *key != &selected_platform)
                .cloned()
                .collect::<Vec<_>>();
            for other in other_platforms {
                entry.remove(&other);
            }
        }
    }
    Ok(expected["tools"].clone())
}
