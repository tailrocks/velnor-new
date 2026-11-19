// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use crate::declared_fixture::{TestResult, rendered_text};
use crate::yaml_tree::document;

mod compare;
mod replaced;
mod scripted;
mod shapes;

use compare::{Pair, compare_head, compare_steps};
use scripted::{Pins, SCRIPT, SCRIPTED, compare_scripted, pinned};

const SCALE_SET: &str = "ubuntu-26.04-scale-set";
const HOSTED: &str = "ubuntu-26.04";

const CHAINS: [&str; 24] = [
    "cache-default-branch-guard",
    "empty-cache-hosted",
    "empty-cache-scale-set",
    "bind-hosted",
    "bind-scale-set",
    "buildx-hosted",
    "buildx-scale-set",
    "compose-hosted",
    "compose-scale-set",
    "expect-fail-hosted",
    "expect-fail-scale-set",
    "mask-hosted",
    "mask-scale-set",
    "pressure-a-hosted",
    "pressure-a-scale-set",
    "pressure-b-hosted",
    "pressure-b-scale-set",
    "pressure-c-hosted",
    "pressure-c-scale-set",
    "submodule-hosted",
    "submodule-scale-set",
    "testcontainers-hosted",
    "testcontainers-scale-set",
    "outputs-scale-set",
];

const NARROWER: [&str; 30] = [
    "cache-default-branch-guard",
    "bind-hosted",
    "bind-scale-set",
    "buildx-hosted",
    "buildx-scale-set",
    "cancel-hosted",
    "cancel-scale-set",
    "cancel-service-hosted",
    "cancel-service-scale-set",
    "container-hosted",
    "container-scale-set",
    "js-pin-hosted",
    "js-pin-scale-set",
    "expect-fail-hosted",
    "expect-fail-scale-set",
    "mask-hosted",
    "mask-scale-set",
    "pressure-a-hosted",
    "pressure-a-scale-set",
    "pressure-b-hosted",
    "pressure-b-scale-set",
    "pressure-c-hosted",
    "pressure-c-scale-set",
    "services-hosted",
    "services-scale-set",
    "compare",
    "outputs-hosted",
    "outputs-scale-set",
    "verify-hosted",
    "verify-scale-set",
];

fn set(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

fn repo(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(path)
}

fn read(path: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(repo(path))?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StepSignature {
    uses: Option<String>,
    run: Option<String>,
    env: BTreeMap<String, String>,
    with: BTreeMap<String, String>,
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

fn table(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|map| map.iter().map(|(k, v)| (k.clone(), text(v))).collect())
        .unwrap_or_default()
}

fn signature(step: &Value, run: Option<String>) -> StepSignature {
    StepSignature {
        uses: step.get("uses").map(text),
        run,
        env: table(step.get("env")),
        with: table(step.get("with")),
    }
}

fn gen_steps(job: &Value) -> Vec<StepSignature> {
    job.get("steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|step| signature(step, step.get("run").map(text)))
        .collect()
}

fn engine_steps(job: &Value) -> (Vec<StepSignature>, Vec<StepSignature>) {
    let mut chains = Vec::new();
    let mut flat = Vec::new();
    for step in job
        .get("steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let run = step.get("run").map(text);
        chains.push(signature(step, run.clone()));
        match run {
            Some(run) => {
                for part in run.split(" && ") {
                    flat.push(signature(step, Some(part.to_owned())));
                }
            }
            None => flat.push(signature(step, None)),
        }
    }
    (chains, flat)
}

fn expression(value: &Value) -> String {
    text(value)
        .trim()
        .trim_start_matches("${{")
        .trim_end_matches("}}")
        .trim()
        .to_owned()
}

fn condition(job: &Value) -> Option<String> {
    job.get("if").map(expression)
}

fn lane(runs_on: Option<&Value>) -> Option<&'static str> {
    let flat = runs_on?.to_string();
    if flat.contains(SCALE_SET) {
        Some("scale-set")
    } else if flat.contains(HOSTED) {
        Some("hosted")
    } else {
        None
    }
}

fn permissions(job: &Value, workflow: &Value) -> BTreeMap<String, String> {
    table(
        job.get("permissions")
            .or_else(|| workflow.get("permissions")),
    )
}

fn modes(condition: &str) -> BTreeSet<String> {
    condition
        .split("inputs.mode == '")
        .skip(1)
        .filter_map(|rest| rest.split('\'').next())
        .map(str::to_owned)
        .collect()
}

fn offered(generated: &Value) -> BTreeSet<String> {
    generated
        .pointer("/on/workflow_dispatch/inputs/mode/options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(text)
        .collect()
}

type Jobs = serde_json::Map<String, Value>;

fn undeclared(
    engine: &Jobs,
    generated: &Jobs,
    offered: &BTreeSet<String>,
    unexpected: &mut Vec<String>,
) {
    for (id, job) in engine {
        let declared = generated.contains_key(id);
        let wanted = condition(job)
            .map(|text| modes(&text))
            .is_some_and(|set| !set.is_empty() && set.is_subset(offered));
        if wanted != declared {
            unexpected.push(format!(
                "{id}: declared {declared}, but its modes {:?} are offered {wanted}",
                condition(job)
            ));
        }
    }
}

#[test]
fn every_declared_qualification_job_matches_the_engine_job_but_for_the_listed_differences()
-> TestResult {
    let config = read(".velnor/workflows.toml")?;
    let generated: Value = document(&rendered_text(&config, "qualification-gen")?)?;
    let engine: Value = document(&read(".github/workflows/qualification.yml")?)?;
    let engine_jobs = engine
        .get("jobs")
        .and_then(Value::as_object)
        .ok_or("no engine jobs")?;
    let generated_jobs = generated
        .get("jobs")
        .and_then(Value::as_object)
        .ok_or("no generated jobs")?;
    let offered = offered(&generated);
    let script = read(SCRIPT)?;
    let mise = read("mise.toml")?;
    let pins = Pins {
        mbx: pinned(&mise, "mr-boxington")?,
        rust: pinned(&mise, "rust")?,
    };
    let mut kinds = BTreeMap::new();
    let mut unexpected = Vec::new();
    for (id, job) in generated_jobs {
        let Some(engine_job) = engine_jobs.get(id) else {
            unexpected.push(format!("{id}: the engine has no such job"));
            continue;
        };
        let pair = Pair {
            id,
            engine: engine_job,
            engine_workflow: &engine,
            generated: job,
            generated_workflow: &generated,
        };
        compare_head(&pair, &mut kinds, &mut unexpected);
        if SCRIPTED.contains(&id.as_str()) {
            compare_scripted(&pair, &script, &pins, &mut unexpected);
        } else {
            compare_steps(&pair, &mut kinds, &mut unexpected);
        }
    }
    undeclared(engine_jobs, generated_jobs, &offered, &mut unexpected);
    assert!(unexpected.is_empty(), "{}", unexpected.join("\n"));
    let scale_sets: BTreeSet<String> = generated_jobs
        .keys()
        .filter(|id| id.ends_with("-scale-set"))
        .cloned()
        .collect();
    let listed = |kind: &str| kinds.get(kind).cloned().unwrap_or_default();
    assert_eq!(listed("runs-on gets the fork fence"), scale_sets);
    shapes::assert_notes(&kinds, &scale_sets);
    assert_eq!(
        listed("a pipe is written as two steps"),
        BTreeSet::from(["outputs-scale-set".to_owned()])
    );
    let empty = set(&["empty-cache-hosted", "empty-cache-scale-set"]);
    assert_eq!(listed("an || is written as one test"), empty);
    assert_eq!(listed("a case condition is parenthesised"), empty);
    assert_eq!(
        listed("a restore-only actions/cache is actions/cache/restore"),
        set(&["cache-hosted"])
    );
    assert_eq!(
        listed("a line feed in a printf format is written as \\n"),
        empty
    );
    assert_eq!(
        listed("a semicolon in a message is a full stop"),
        set(&["cache-default-branch-guard"])
    );
    assert_eq!(listed("an && chain is written as steps"), set(&CHAINS));
    assert_eq!(listed("permissions are narrower"), set(&NARROWER));
    Ok(())
}

#[test]
fn the_committed_workflow_and_the_sample_are_what_the_repository_config_renders() -> TestResult {
    let config = read(".velnor/workflows.toml")?;
    let rendered = rendered_text(&config, "qualification-gen")?;
    assert_eq!(read(".github/workflows/qualification-gen.yml")?, rendered);
    assert_eq!(
        read("generator/samples/qualification/view/.velnor/workflows.toml")?,
        config
    );
    assert_eq!(
        read("generator/samples/qualification/expected/.github/workflows/qualification-gen.yml")?,
        rendered
    );
    Ok(())
}

#[test]
fn the_mbx_script_parses_and_refuses_an_unknown_phase() -> TestResult {
    let script = repo(SCRIPT);
    let parsed = std::process::Command::new("/bin/bash")
        .arg("-n")
        .arg(&script)
        .status()?;
    assert!(parsed.success());
    let unknown = std::process::Command::new("/bin/bash")
        .arg(&script)
        .arg("no-such-phase")
        .output()?;
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("no-such-phase"));
    Ok(())
}
