// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use velnor_actions_contract::{Job, StepId, StepKind, StepRole};
use velnor_actions_orchestrator::{finalized_jobs, prepare};

use super::one_job_per_crate::mbx_repo;
use super::workflow_configuration_fixture::{git, write_nextest_task};

pub(super) const WORKSPACE: &str = "ws";
const BIN: &str = "\n[stacks.rust.state_slices]\nsmall = [\"bina\", \"binb\"]\n";
const MEMBERS: [(&str, &str); 7] = [
    ("bina", ""),
    ("binb", ""),
    ("builddep", ""),
    ("devdep", ""),
    ("leaf", ""),
    ("mid", "\n[dependencies]\nleaf = { path = \"../leaf\" }\n"),
    (
        "other",
        "\n[dev-dependencies]\ndevdep = { path = \"../devdep\" }\n\n[build-dependencies]\nbuilddep = { path = \"../builddep\" }\n",
    ),
];
const LOCKED_EDGES: [(&str, &[&str]); 2] = [("mid", &["leaf"]), ("other", &["builddep", "devdep"])];

pub(super) fn sliced_repo(slices: &str) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo = mbx_repo(false)?;
    let root = repo.path();
    let ws = root.join(WORKSPACE);
    let listed = MEMBERS
        .iter()
        .map(|(name, _)| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::create_dir_all(&ws)?;
    fs::write(
        ws.join("Cargo.toml"),
        format!("[workspace]\nmembers = [{listed}]\nresolver = \"2\"\n"),
    )?;
    let mut lock = vec!["version = 4\n".to_owned()];
    for (name, deps) in MEMBERS {
        fs::create_dir_all(ws.join(name).join("src"))?;
        fs::write(
            ws.join(name).join("Cargo.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{deps}"
            ),
        )?;
        fs::write(ws.join(name).join("src/lib.rs"), "pub fn f() {}\n")?;
        let edges = LOCKED_EDGES
            .iter()
            .find(|(owner, _)| *owner == name)
            .map_or(String::new(), |(_, to)| {
                let list: Vec<String> = to.iter().map(|dep| format!(" \"{dep}\",")).collect();
                format!("dependencies = [\n{}\n]\n", list.join("\n"))
            });
        lock.push(format!(
            "[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n{edges}"
        ));
    }
    fs::write(ws.join("Cargo.lock"), lock.join("\n"))?;
    fs::write(
        ws.join("mise.toml"),
        "wrappers = { cargo = { command = \"mbx\", env = { MBX_CARGO_SHIM_MODE = \"1\" } } }\n",
    )?;
    write_nextest_task(&ws)?;
    let config = root.join(".velnor/config.toml");
    let mut text = fs::read_to_string(&config)?;
    text.push_str(slices);
    fs::write(&config, text)?;
    Ok(repo)
}

pub(super) fn binned_repo() -> Result<tempfile::TempDir, Box<dyn Error>> {
    sliced_repo(BIN)
}

fn shell_of(job: &Job, id: StepId) -> Result<(String, BTreeMap<String, String>), Box<dyn Error>> {
    job.steps
        .iter()
        .find(|step| step.id == Some(id))
        .and_then(|step| match &step.kind {
            StepKind::Shell { run, env } => Some((run.get(2)?.clone(), env.clone())),
            _ => None,
        })
        .ok_or_else(|| format!("no {id:?} shell step").into())
}

fn run_shell(
    script: &str,
    env: &[(String, String)],
    repo: &Path,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let temp = repo.join(".git/velnor-runner-temp");
    fs::create_dir_all(&temp)?;
    let output = temp.join("github_output");
    fs::write(&output, "")?;
    let record = temp.join("velnor/state-discovery.record");
    if record.exists() {
        fs::remove_file(&record)?;
    }
    let run = Command::new("sh")
        .args(["-c", script])
        .env_remove("GITHUB_REPOSITORY")
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_WORKSPACE", repo)
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/testmain")
        .env("GITHUB_REF_PROTECTED", "true")
        .env("ImageOS", "ubuntu24")
        .envs(env.iter().map(|(name, value)| (name, value)))
        .output()?;
    if !run.status.success() {
        return Err(format!("script failed: {}", String::from_utf8_lossy(&run.stderr)).into());
    }
    let mut values: BTreeMap<String, String> = fs::read_to_string(&output)?
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    if let Ok(record) = fs::read_to_string(&record) {
        for line in record.lines() {
            if let Some((name, value)) = line.split_once('=') {
                values.insert(format!("record.{name}"), value.to_owned());
            }
        }
    }
    Ok(values)
}

fn expression_name(value: &str, context: &str) -> Option<String> {
    value
        .trim_start_matches("${{")
        .trim_end_matches("}}")
        .trim()
        .strip_prefix(context)
        .map(str::to_owned)
}

pub(super) fn plan_outputs(
    jobs: &BTreeMap<String, Job>,
    repo: &Path,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    git(&["add", "-A"], repo)?;
    let plan = jobs.get("plan").ok_or("no plan job")?;
    let (script, env) = shell_of(plan, StepId::CacheKeys)?;
    let env: Vec<(String, String)> = env
        .into_iter()
        .map(|(name, value)| {
            let value = if name == "VELNOR_DEFAULT_REF" {
                "refs/heads/testmain".to_owned()
            } else {
                value
            };
            (name, value)
        })
        .collect();
    run_shell(&script, &env, repo)
}

fn seal_matrix(
    jobs: &BTreeMap<String, Job>,
    repo: &Path,
    keys: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, serde_json::Value>, Box<dyn Error>> {
    let plan = jobs.get("plan").ok_or("no plan job")?;
    let (script, env) = shell_of(plan, StepId::StateSelect)?;
    let env: Vec<(String, String)> = env
        .into_iter()
        .map(|(name, value)| {
            let resolved = expression_name(&value, "steps.cache-keys.outputs.")
                .and_then(|output| keys.get(&output).cloned())
                .unwrap_or_else(|| {
                    if value.contains("${{") {
                        String::new()
                    } else {
                        value
                    }
                });
            (name, resolved)
        })
        .collect();
    let outputs = run_shell(&script, &env, repo)?;
    let matrix: serde_json::Value = serde_json::from_str(
        outputs
            .get("state_matrix_seal")
            .ok_or("the select step wrote no seal matrix")?,
    )?;
    Ok(matrix["include"]
        .as_array()
        .ok_or("the seal matrix has no include list")?
        .iter()
        .filter_map(|leg| Some((leg["slice"].as_str()?.to_owned(), leg.clone())))
        .collect())
}

fn consumer_record(
    job: &Job,
    repo: &Path,
    keys: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let (script, env) = shell_of(job, StepId::MbxBundleKey)?;
    let env: Vec<(String, String)> = env
        .into_iter()
        .map(|(name, value)| {
            let resolved = match name.as_str() {
                "VELNOR_DEFAULT_REF" => "refs/heads/testmain".to_owned(),
                "VELNOR_MBX_BUNDLE_VERSION" | "VELNOR_MBX_BUNDLE_TOOLCHAIN" => keys
                    .get(&format!("env.{name}"))
                    .cloned()
                    .unwrap_or_default(),
                _ => expression_name(&value, "needs.plan.outputs.")
                    .map(|output| keys.get(&output).cloned().unwrap_or_default())
                    .unwrap_or(value),
            };
            (name, resolved)
        })
        .collect();
    run_shell(&script, &env, repo)
}

fn with_plan_env(
    jobs: &BTreeMap<String, Job>,
    mut keys: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let plan = jobs.get("plan").ok_or("no plan job")?;
    let (_, env) = shell_of(plan, StepId::CacheKeys)?;
    for name in ["VELNOR_MBX_BUNDLE_VERSION", "VELNOR_MBX_BUNDLE_TOOLCHAIN"] {
        keys.insert(
            format!("env.{name}"),
            env.get(name).cloned().ok_or("plan lost its MBX env")?,
        );
    }
    Ok(keys)
}

fn output_tokens(job: &Job) -> Vec<String> {
    let mut tokens = Vec::new();
    for step in &job.steps {
        let StepKind::Shell { env, .. } = &step.kind else {
            continue;
        };
        for value in env.values() {
            let mut rest = value.as_str();
            while let Some(at) = rest.find("state_slice_") {
                rest = &rest[at + "state_slice_".len()..];
                let end = ["_attested", "_snapshot", "_compat"]
                    .iter()
                    .filter_map(|suffix| rest.find(suffix))
                    .min()
                    .unwrap_or(rest.len());
                tokens.push(rest[..end].to_owned());
            }
        }
    }
    tokens
}

#[test]
fn binned_jobs_share_one_slice_and_read_only_their_own_outputs() -> Result<(), Box<dyn Error>> {
    let repo = binned_repo()?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    let keys = with_plan_env(&jobs, plan_outputs(&jobs, repo.path())?)?;
    for (job_id, slice) in [
        ("rust-bina", "small"),
        ("rust-binb", "small"),
        ("rust-leaf", "rust-leaf"),
    ] {
        let job = jobs
            .get(job_id)
            .ok_or_else(|| format!("{job_id} missing"))?;
        let record = consumer_record(job, repo.path(), &keys)?;
        assert_eq!(
            record.get("record.unit").map(String::as_str),
            Some(format!("slice-{slice}").as_str()),
            "{job_id} restores the wrong unit"
        );
        let own = slice.replace('-', "_");
        let tokens = output_tokens(job);
        assert!(!tokens.is_empty(), "{job_id} reads no slice output");
        assert!(
            tokens.iter().all(|token| *token == own),
            "{job_id} reads another slice's output: {tokens:?}"
        );
    }
    Ok(())
}

#[test]
fn a_seal_leg_saves_under_the_prefix_its_consumers_restore() -> Result<(), Box<dyn Error>> {
    let repo = binned_repo()?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    let keys = with_plan_env(&jobs, plan_outputs(&jobs, repo.path())?)?;
    let legs = seal_matrix(&jobs, repo.path(), &keys)?;
    let seal = jobs.get("state-seal").ok_or("no seal job")?;
    let template = seal
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::StateSealSubject))
        .and_then(|step| match &step.kind {
            StepKind::Shell { env, .. } => env.get("VELNOR_STATE_SEAL_PREFIX").cloned(),
            _ => None,
        })
        .ok_or("the seal subject lost its prefix")?;
    for (consumer, slice) in [("rust-binb", "small"), ("rust-mid", "rust-mid")] {
        let leg = legs
            .get(slice)
            .ok_or_else(|| format!("no seal leg for {slice}"))?;
        let field = |name: &str| leg[name].as_str().unwrap_or_default().to_owned();
        let saved = template
            .replace("${{ matrix.slice }}", &field("slice"))
            .replace("${{ matrix.compat }}", &field("compat"))
            .replace("${{ matrix.snapshot }}", &field("snapshot"));
        let job = jobs
            .get(consumer)
            .ok_or_else(|| format!("{consumer} missing"))?;
        let record = consumer_record(job, repo.path(), &keys)?;
        assert_eq!(
            record.get("record.snapshot_prefix"),
            Some(&saved),
            "{consumer} restores a prefix its seal leg never saves"
        );
    }
    Ok(())
}

#[test]
fn every_cargo_root_gets_one_producer_job_whatever_its_slice_count() -> Result<(), Box<dyn Error>> {
    let repo = binned_repo()?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    let producers: Vec<&String> = jobs
        .keys()
        .filter(|id| id.starts_with("state-producer-"))
        .collect();
    assert_eq!(
        producers,
        ["state-producer-crates-nested", "state-producer-ws"],
        "one producer job per Cargo root"
    );
    let legs: Vec<u32> = producers
        .iter()
        .filter_map(|id| jobs[*id].matrix.as_ref().map(|matrix| matrix.legs))
        .collect();
    assert_eq!(legs, [1, 6], "a leg per slice of each root");
    Ok(())
}

#[test]
fn state_slices_reject_unknown_shared_cross_root_and_job_named_bins() -> Result<(), Box<dyn Error>>
{
    let accepted = binned_repo()?;
    assert!(prepare(accepted.path()).is_ok(), "a valid bin was refused");
    for (slices, needle) in [
        (
            "\n[stacks.rust.state_slices]\nsmall = [\"bina\", \"nosuch\"]\n",
            "unknown_rust_package:nosuch",
        ),
        (
            "\n[stacks.rust.state_slices]\na = [\"bina\"]\nb = [\"bina\"]\n",
            "package_in_two_state_slices:bina",
        ),
        (
            "\n[stacks.rust.state_slices]\nmixed = [\"bina\", \"nested\"]\n",
            "state_slice_spans_cargo_roots",
        ),
        (
            "\n[stacks.rust.state_slices]\nrust-leaf = [\"bina\"]\n",
            "state_slice_names_a_crate_job",
        ),
    ] {
        let repo = sliced_repo(slices)?;
        let message = match prepare(repo.path()) {
            Ok(_) => return Err(format!("{slices:?} was accepted").into()),
            Err(error) => error.to_string(),
        };
        assert!(message.contains(needle), "{slices:?}: {message}");
    }
    Ok(())
}
