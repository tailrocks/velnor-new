// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};
use crate::yaml_tree::document;

const ON: &[u8] = b"[cache]\nmbx_remote = \"github-actions\"\n";
const ACTION: &str = "jdx/mr-boxington-action@";

fn lanes(config: &[u8]) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let text = workflow(&view(&["Cargo.toml"], Some(config))?)?;
    Ok(document(&text)?)
}

#[test]
fn the_remote_cache_is_off_unless_opted_in_and_off_output_is_unchanged() -> TestResult {
    let plain = workflow(&view(&["Cargo.toml"], None)?)?;
    assert!(!plain.contains("mr-boxington"));
    for config in [&b"[cache]\n"[..], b"cache_debug = false\n"] {
        assert_eq!(workflow(&view(&["Cargo.toml"], Some(config))?)?, plain);
    }
    Ok(())
}

#[test]
fn every_cargo_job_restores_the_mbx_cache_after_tools_and_before_repository_code() -> TestResult {
    for shape in ["", "shape = 'groups'\n"] {
        let config = [shape.as_bytes(), ON].concat();
        let yaml = lanes(&config)?;
        let mut configured = 0;
        for (id, job) in yaml["jobs"].as_object().ok_or("no jobs")? {
            let steps = job["steps"].as_array().ok_or("no steps")?;
            let at = |pred: &dyn Fn(&serde_json::Value) -> bool| steps.iter().position(pred);
            let uses = |step: &serde_json::Value, prefix: &str| {
                step["uses"].as_str().is_some_and(|u| u.starts_with(prefix))
            };
            let cargo = at(&|s| s["run"].as_str().is_some_and(|r| r.contains(" run -- ")));
            let Some(mbx) = at(&|s| uses(s, ACTION)) else {
                assert!(cargo.is_none(), "{id} runs cargo without the mbx cache");
                continue;
            };
            configured += 1;
            let tools = at(&|s| uses(s, "jdx/mise-action@")).ok_or("no tools step")?;
            assert!(tools < mbx && mbx < cargo.ok_or("no cargo")?, "{id}");
            assert!(
                steps[..mbx]
                    .iter()
                    .all(|s| s["run"].as_str().is_none_or(|r| !r.contains(" run -- "))),
                "{id}: repository code runs before the cache setup"
            );
            assert_eq!(steps.iter().filter(|s| uses(s, ACTION)).count(), 1, "{id}");
        }
        assert_ne!(configured, 0);
    }
    Ok(())
}

#[test]
fn the_action_is_pinned_by_full_sha_and_only_a_push_to_the_default_branch_can_save() -> TestResult {
    let yaml = lanes(ON)?;
    let mut seen = 0;
    for job in yaml["jobs"].as_object().ok_or("no jobs")?.values() {
        for step in job["steps"].as_array().ok_or("no steps")? {
            let Some(uses) = step["uses"].as_str().and_then(|u| u.strip_prefix(ACTION)) else {
                continue;
            };
            seen += 1;
            assert!(
                uses.len() == 40 && uses.bytes().all(|b| b.is_ascii_hexdigit()),
                "{uses}"
            );
            let with = &step["with"];
            assert_eq!(with["backend"], "github");
            for key in [
                "save-on-pull-request",
                "save-on-protected-branch",
                "save-on-workflow-dispatch",
            ] {
                assert_eq!(with[key], "false", "{key}");
            }
            assert!(with.get("token").is_none() && with.get("github-token").is_none());
        }
    }
    assert_ne!(seen, 0);
    Ok(())
}

#[test]
fn opting_in_grants_no_write_or_identity_token_and_reads_no_secret() -> TestResult {
    let text = workflow(&view(&["Cargo.toml"], Some(ON))?)?;
    let yaml: serde_json::Value = document(&text)?;
    assert_eq!(yaml["permissions"], serde_json::json!({"contents": "read"}));
    for job in yaml["jobs"].as_object().ok_or("no jobs")?.values() {
        let steps = job["steps"].as_array().ok_or("no steps")?;
        if steps
            .iter()
            .any(|s| s["uses"].as_str().is_some_and(|u| u.starts_with(ACTION)))
        {
            let permissions = job.get("permissions").and_then(|p| p.as_object());
            assert!(permissions.is_none_or(|p| p.values().all(|v| v == "read")));
        }
    }
    assert!(!text.contains("id-token") && !text.contains("secrets."));
    assert!(!text.contains("MBX_REMOTE"));
    Ok(())
}

#[test]
fn an_unknown_backend_or_key_under_cache_is_refused() -> TestResult {
    for config in [
        &b"[cache]\nmbx_remote = \"s3\"\n"[..],
        b"[cache]\nmbx_remote = true\n",
        b"[cache]\nmbx = \"github-actions\"\n",
    ] {
        assert!(workflow(&view(&["Cargo.toml"], Some(config))?).is_err());
    }
    Ok(())
}

const GIT_LOCK: &[u8] = b"version = 4\n\n[[package]]\nname = \"shared-lib\"\nversion = \"0.1.0\"\nsource = \"git+https://github.com/example/shared-lib?branch=main#0123456789abcdef\"\n";
const REGISTRY_CONFIG: &[u8] =
    b"[registries.private]\nindex = \"sparse+https://registry.example.invalid/\"\n";

fn private_view(config: &[u8]) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let mut view = view(&["Cargo.toml"], Some(config))?;
    let lock = velnor_model::RepoPath::new("Cargo.lock")?;
    view.files.insert(lock, GIT_LOCK.to_vec());
    Ok(view)
}

#[test]
fn a_private_dependency_refuses_the_remote_cache_a_fork_pull_request_could_restore() -> TestResult {
    let error = workflow(&private_view(ON)?).expect_err("private sources and the mbx cache");
    let text = error.to_string();
    assert!(text.contains("cache.mbx_remote"), "{text}");
    assert!(text.contains("private dependencies"), "{text}");
    assert!(text.contains("shared-lib"), "{text}");

    let mut configured = view(&["Cargo.toml"], Some(ON))?;
    configured.files.insert(
        velnor_model::RepoPath::new(".cargo/config.toml")?,
        REGISTRY_CONFIG.to_vec(),
    );
    let error = workflow(&configured).expect_err("a private registry and the mbx cache");
    assert!(error.to_string().contains("cache.mbx_remote"), "{error}");
    Ok(())
}

#[test]
fn a_private_dependency_without_the_opt_in_still_generates_and_never_restores_remotely()
-> TestResult {
    let text = workflow(&private_view(b"")?)?;
    assert!(!text.contains("mr-boxington"));
    Ok(())
}
