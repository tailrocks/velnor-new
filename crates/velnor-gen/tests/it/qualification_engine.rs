// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use crate::declared_fixture::{TestResult, rendered_text};
use crate::yaml_tree::document;

mod cache;
mod lists;
mod scripted;
mod shapes;

const SCALE_SET: &str = "ubuntu-26.04-scale-set";
const HOSTED: &str = "ubuntu-26.04";

const PIPE_REPLACED: [(&str, [&str; 2]); 1] = [(
    "g4-path-ok | grep -qx path-ok",
    [
        "g4-path-ok > \"$RUNNER_TEMP/g4-path-out\"",
        "grep -qx path-ok \"$RUNNER_TEMP/g4-path-out\"",
    ],
)];

fn set(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

fn repo(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
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

fn replace_pipes(steps: Vec<StepSignature>) -> (Vec<StepSignature>, bool) {
    let mut replaced = false;
    let mut out = Vec::new();
    for step in steps {
        let hit = PIPE_REPLACED
            .iter()
            .find(|(from, _)| step.run.as_deref() == Some(from));
        match hit {
            Some((_, to)) => {
                replaced = true;
                for run in to {
                    out.push(StepSignature {
                        run: Some((*run).to_owned()),
                        ..step.clone()
                    });
                }
            }
            None => out.push(step),
        }
    }
    (out, replaced)
}

fn condition(job: &Value) -> Option<String> {
    job.get("if").map(|value| {
        text(value)
            .trim()
            .trim_start_matches("${{")
            .trim_end_matches("}}")
            .trim()
            .to_owned()
    })
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

fn compare(
    id: &str,
    engine: &Value,
    engine_workflow: &Value,
    generated: &Value,
    generated_workflow: &Value,
    kinds: &mut BTreeMap<&'static str, BTreeSet<String>>,
    unexpected: &mut Vec<String>,
) {
    let mut note = |kind: &'static str| {
        kinds.entry(kind).or_default().insert(id.to_owned());
    };
    let (unrolled, looped) = shapes::unroll(engine);
    let engine = &unrolled;
    if looped {
        note("a retry loop is written as plain probes");
    }
    shapes::compare(id, engine, generated, &mut note, unexpected);
    for key in ["name", "timeout-minutes", "needs", "outputs"] {
        if engine.get(key) != generated.get(key) {
            unexpected.push(format!(
                "{id}: {key}: engine {:?}, generated {:?}",
                engine.get(key),
                generated.get(key)
            ));
        }
    }
    cache::head(
        id,
        [engine, engine_workflow, generated],
        &mut note,
        unexpected,
    );
    let engine_lane = lane(engine.get("runs-on"));
    if engine_lane.is_none() || engine_lane != lane(generated.get("runs-on")) {
        unexpected.push(format!(
            "{id}: runs-on: engine {:?}, generated {:?}",
            engine.get("runs-on"),
            generated.get("runs-on")
        ));
    }
    if engine_lane == Some("scale-set") {
        note("runs-on gets the fork fence");
    }
    if engine.pointer("/defaults/run/shell").is_some() {
        note("defaults.run.shell is not written");
    }
    let from_engine = permissions(engine, engine_workflow);
    let from_generated = permissions(generated, generated_workflow);
    if from_engine != from_generated {
        if from_generated
            .iter()
            .all(|(scope, level)| from_engine.get(scope) == Some(level))
        {
            note("permissions are narrower");
        } else {
            unexpected.push(format!(
                "{id}: permissions: engine {from_engine:?}, generated {from_generated:?}"
            ));
        }
    }
    if scripted::SCRIPTED.contains(&id) {
        note("a script replaces inline commands");
        scripted::scripted_steps(id, engine, generated, unexpected);
        return;
    }
    let (chains, flat) = engine_steps(engine);
    let (flat, piped) = replace_pipes(flat);
    let (flat, applied) = cache::apply(id, flat);
    if flat != gen_steps(generated) {
        unexpected.push(format!(
            "{id}: steps: engine {flat:#?}, generated {:#?}",
            gen_steps(generated)
        ));
    }
    if piped {
        note("a pipe is written as two steps");
    }
    for kind in applied {
        note(kind);
    }
    if chains.len()
        != engine
            .get("steps")
            .map_or(0, |steps| steps.as_array().map_or(0, Vec::len))
        || flat.len() != chains.len()
    {
        note("an && chain is written as steps");
    }
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
    let mut kinds = BTreeMap::new();
    let mut unexpected = Vec::new();
    for (id, job) in generated_jobs {
        match engine_jobs.get(id) {
            Some(engine_job) => compare(
                id,
                engine_job,
                &engine,
                job,
                &generated,
                &mut kinds,
                &mut unexpected,
            ),
            None => unexpected.push(format!("{id}: the engine has no such job")),
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
    assert_eq!(
        listed("a script replaces inline commands"),
        set(&scripted::SCRIPTED)
    );
    assert_eq!(
        listed("an && chain is written as steps"),
        set(&lists::CHAINS)
    );
    assert_eq!(listed("permissions are narrower"), set(&lists::NARROWER));
    cache::assert_notes(&kinds);
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
