// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::scoped_workspace::{commit, generate_lockfile, plan, put};
use super::source_qualification::{WORKFLOW_PATH, generator_repo};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{error::Error, fs, path::Path, process::Command};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

pub(super) fn fixture() -> Result<(tempfile::TempDir, String), Box<dyn Error>> {
    let repo = generator_repo(true)?;
    let root = repo.path();
    put(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/cli\", \"crates/orchestrator\", \"crates/guard\"]\nresolver = \"2\"\n",
    )?;
    for (name, manifest) in [
        (
            "cli",
            "[package]\nname = \"velnor-actions-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\nvelnor-actions-orchestrator = { path = \"../orchestrator\" }\n",
        ),
        (
            "orchestrator",
            "[package]\nname = \"velnor-actions-orchestrator\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\nvelnor-archive-guard = { path = \"../guard\" }\n",
        ),
        (
            "guard",
            "[package]\nname = \"velnor-archive-guard\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
    ] {
        put(root, &format!("crates/{name}/Cargo.toml"), manifest)?;
        put(
            root,
            &format!("crates/{name}/src/lib.rs"),
            "pub fn f() {}\n",
        )?;
    }
    let config = root.join(".velnor/config.toml");
    let old = fs::read_to_string(&config)?;
    fs::write(
        config,
        format!(
            "{old}\n[stacks.rust]\nnon_inputs = [\"docs/**\"]\n[[stacks.rust.inputs]]\nglobs = [\"fixtures/**\", \"scripts/**\", \".github/**\", \".velnor/**\", \"crates/**\", \"Cargo.toml\", \"Cargo.lock\"]\npackages = [\"velnor-archive-guard\"]\ncomplete = true\n"
        ),
    )?;
    put(root, "docs/guide.md", "old\n")?;
    put(root, "fixtures/oracle.txt", "old\n")?;
    put(root, "scripts/phase.sh", "old\n")?;
    generate_lockfile(root)?;
    let base = commit(root, "base")?;
    Ok((repo, base))
}

pub(super) fn install_selector(root: &Path) -> Result<(), Box<dyn Error>> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/source-qualification-routing.py");
    let target = root.join("scripts/generator-release/source-qualification-routing.py");
    fs::create_dir_all(target.parent().ok_or("missing parent")?)?;
    fs::copy(source, target)?;
    Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[(byte >> 4) as usize]),
                char::from(HEX[(byte & 15) as usize]),
            ]
        })
        .collect()
}

fn prepare_artifact(
    temp: &Path,
    plan: &Value,
    mutation: &str,
) -> Result<(String, String), Box<dyn Error>> {
    let artifact = temp.join("source-qualification-plan");
    fs::create_dir(&artifact)?;
    let mut admitted_plan = plan.clone();
    if [
        "job_missing",
        "job_duplicate",
        "lane_wrong",
        "lane_disposition",
    ]
    .contains(&mutation)
    {
        let jobs = admitted_plan["jobs"].as_array_mut().ok_or("jobs")?;
        let position = jobs
            .iter()
            .position(|job| {
                job["job_id"].as_str().is_some_and(|id| {
                    id == "rust-velnor-actions-cli" || id == "rust-velnor-actions-cli__hosted"
                })
            })
            .ok_or("CLI job")?;
        if mutation == "job_missing" {
            jobs.remove(position);
        } else if mutation == "job_duplicate" {
            jobs.push(jobs[position].clone());
        } else if mutation == "lane_wrong" {
            jobs[position]["lane"]["logical_job_id"] = json!("rust-wrong");
        } else {
            jobs[position]["disposition"] = json!("not_affected");
        }
    }
    let bytes = serde_json::to_vec(&admitted_plan)?;
    let sha = hex_digest(&bytes);
    fs::write(artifact.join("plan.json"), &bytes)?;
    let matrix = if mutation == "matrix_mismatch" {
        serde_json::to_vec(&json!({"include": ["unexpected"]}))?
    } else {
        serde_json::to_vec(&plan["matrix"])?
    };
    let matrix_sha = hex_digest(&matrix);
    fs::write(artifact.join("matrix.json"), &matrix)?;
    if mutation == "matrix_tampered" {
        fs::write(artifact.join("matrix.json"), b"{}")?;
    }
    if mutation == "matrix_missing" {
        fs::remove_file(artifact.join("matrix.json"))?;
    }
    if mutation == "extra" {
        fs::write(artifact.join("unexpected"), "extra")?;
    }
    if mutation == "tampered" {
        fs::write(artifact.join("plan.json"), b"{}")?;
    }
    if mutation == "missing" {
        fs::remove_file(artifact.join("plan.json"))?;
    }
    Ok((sha, matrix_sha))
}

fn cli_routes(plan: &Value, mutation: &str) -> Result<Value, Box<dyn Error>> {
    if mutation == "route" {
        return Ok(json!(["rust-missing"]));
    }
    Ok(Value::Array(
        plan["jobs"]
            .as_array()
            .ok_or("jobs")?
            .iter()
            .filter_map(|job| {
                job["job_id"]
                    .as_str()
                    .filter(|id| {
                        [
                            "rust-velnor-actions-cli",
                            "rust-velnor-actions-cli__hosted",
                            "rust-velnor-actions-cli__local",
                        ]
                        .contains(id)
                    })
                    .map(|id| json!(id))
            })
            .collect(),
    ))
}

pub(super) fn run_selector(
    root: &Path,
    command: &str,
    plan: &Value,
    mutation: &str,
) -> Result<(bool, String), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let (sha, matrix_sha) = prepare_artifact(temp.path(), plan, mutation)?;
    let event = temp.path().join("event.json");
    let base = if mutation == "base" {
        "wrong"
    } else {
        plan["base"].as_str().ok_or("base")?
    };
    fs::write(
        &event,
        serde_json::to_vec(
            &json!({"repository":{"id":if mutation == "repo_mismatch" { json!(42) } else { json!(41) }},
                "pull_request":{"base":{"sha":base,"repo":{"id":41}},"head":{"sha":plan["head"],"repo":{"id":if mutation == "head_missing" { Value::Null } else if mutation == "head_string" { json!("41") } else { json!(41) }}}}}),
        )?,
    )?;
    let output = temp.path().join("output");
    fs::write(&output, "")?;
    let binary = if mutation == "binary" {
        "0".repeat(64)
    } else {
        plan["generator"]["sha256"]
            .as_str()
            .ok_or("binary")?
            .to_owned()
    };
    let route = cli_routes(plan, mutation)?;
    let status = Command::new("sh")
        .args(["-e", "-c", command])
        .current_dir(root)
        .env("RUNNER_TEMP", temp.path())
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_EVENT_PATH", event)
        .env("GITHUB_RUN_ID", "190001")
        .env(
            "GITHUB_RUN_ATTEMPT",
            if mutation == "stale" { "2" } else { "1" },
        )
        .env(
            "PLAN_RESULT",
            if mutation == "failed" {
                "failure"
            } else {
                "success"
            },
        )
        .env(
            "PLAN_SHA256",
            if mutation == "hash_missing" { "" } else { &sha },
        )
        .env("BINARY_SHA256", binary)
        .env(
            "MATRIX_SHA256",
            if mutation == "matrix_hash_missing" {
                ""
            } else {
                &matrix_sha
            },
        )
        .env(
            "CONTROLLER_VERSION",
            plan["generator"]["version"].as_str().ok_or("version")?,
        )
        .env(
            "CONTROLLER_TARGET",
            plan["generator"]["target"].as_str().ok_or("target")?,
        )
        .env("CLI_ROUTES", serde_json::to_string(&route)?)
        .env("GITHUB_OUTPUT", &output)
        .output()?;
    Ok((status.status.success(), fs::read_to_string(output)?))
}

#[test]
fn source_qualification_routes_real_plans_and_refuses_unbound_artifacts()
-> Result<(), Box<dyn Error>> {
    for (path, expected, operation) in [
        ("docs/guide.md", "skip", "write"),
        ("fixtures/oracle.txt", "run", "write"),
        ("scripts/phase.sh", "run", "write"),
        ("crates/guard/src/lib.rs", "run", "write"),
        ("fixtures/oracle.txt", "run", "rename"),
        ("fixtures/oracle.txt", "run", "delete"),
        ("fixtures/oracle.txt", "run", "mixed"),
        ("unclassified-root-file", "run", "write"),
    ] {
        let (repo, _) = fixture()?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let workflow: Value = serde_saphyr::from_str(tree.get(WORKFLOW_PATH).ok_or("workflow")?)?;
        let steps = workflow["jobs"]["selection"]["steps"]
            .as_array()
            .ok_or("no routed selection")?;
        let command = steps
            .iter()
            .find(|step| step["name"] == "Admit source qualification selection")
            .and_then(|step| step["run"].as_str())
            .ok_or("selector")?;
        install_selector(repo.path())?;
        let base = commit(repo.path(), "with selector")?;
        match operation {
            "rename" => fs::rename(
                repo.path().join(path),
                repo.path().join("fixtures/renamed.txt"),
            )?,
            "delete" => fs::remove_file(repo.path().join(path))?,
            "mixed" => {
                put(repo.path(), "docs/guide.md", "changed\n")?;
                put(repo.path(), path, "changed\n")?;
            }
            _ => put(repo.path(), path, "changed\n")?,
        }
        let head = commit(repo.path(), "change")?;
        let run = plan(repo.path(), "pull_request", Some(&base), &head)?;
        let (success, output) = run_selector(repo.path(), command, &run.value["plan"], "")?;
        assert!(success, "{path}");
        assert_eq!(output.trim(), format!("decision={expected}"));
        if expected == "skip" {
            for mutation in [
                "tampered",
                "missing",
                "base",
                "binary",
                "route",
                "stale",
                "failed",
                "hash_missing",
                "matrix_tampered",
                "matrix_missing",
                "extra",
                "matrix_mismatch",
                "matrix_hash_missing",
                "job_missing",
                "job_duplicate",
                "repo_mismatch",
                "head_missing",
                "head_string",
            ] {
                assert!(
                    !run_selector(repo.path(), command, &run.value["plan"], mutation)?.0,
                    "{mutation}"
                );
            }
        }
    }
    Ok(())
}
