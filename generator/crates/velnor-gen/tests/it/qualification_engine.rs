// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use crate::declared_fixture::{TestResult, rendered_text};
use crate::yaml_tree::document;

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

const CHAINS: [&str; 23] = [
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
    "secret-hosted",
    "secret-scale-set",
];

const NARROWER: [&str; 31] = [
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
    "secret-hosted",
    "secret-scale-set",
    "verify-hosted",
    "verify-scale-set",
];

/// Jobs whose inline scripts are calls of a checked-in script under `qualification/`: velnor-gen
/// refuses a multi-line `run`, a loop, a pipe, a `shell` key and a job `env`.
const SCRIPTED: [&str; 8] = [
    "ports-a-hosted",
    "ports-a-scale-set",
    "ports-b-hosted",
    "ports-b-scale-set",
    "mise-pin-linux-x64",
    "stable-sources-producer-linux",
    "stable-sources-linux-warm",
    "stable-sources-linux-cold",
];

/// Commands of the engine that the script must still hold, verbatim.
const SCRIPT_KEEPS: [(&str, &[&str]); 3] = [
    (
        "qualification/ports.sh",
        &[
            "alpine:3.24.2@sha256:d56c381f961d307a21b3ca004cf1e3910f106644aefb1f43e654c8a56c4fd395",
            "docker run -d --name g4-hold -p 8080:",
            "docker port g4-hold 80 | grep -q 8080",
            "echo port-held",
            "sleep 120",
            "sleep 45",
            "docker rm -f g4-hold",
        ],
    ),
    (
        "qualification/stable-sources.sh",
        &[
            "bash scripts/generator-release/stable-source-producer.sh",
            "/usr/bin/python3 -I -S scripts/generator-release/stable-source-admission.py",
            "bash scripts/generator-release/stable-source-consumer.sh",
            "CARGO_NET_OFFLINE=true bash scripts/build-owned-archive-guard.sh --source-cache \"$CARGO_HOME\"",
            "grep -Fx \"fetch_mode=Cargo.toml:$mode\"",
            "mise --no-config --no-env --no-hooks exec gh@2.104.0 -- gh \"$@\"",
            "[ \"$VELNOR_EVIDENCE_ARTIFACT_ID\" -le 9007199254740991 ]",
        ],
    ),
    (
        "qualification/mise-pin.sh",
        &["binary=\"$(command -v mise)\"", "test \"$actual\" = \"$MISE_SHA256\""],
    ),
];

/// The engine splices the Rust version into the verifier text from the outer shell; the script reads it.
const SPLICES: [(&str, &str); 2] = [
    (
        "'\"${VELNOR_TOOL_RUST+VELNOR_TOOL_RUST=\"$VELNOR_TOOL_RUST\"}\"'",
        "${VELNOR_TOOL_RUST+VELNOR_TOOL_RUST=\"$VELNOR_TOOL_RUST\"}",
    ),
    ("v=\"'\"${VELNOR_TOOL_RUST}\"'\"", "v=\"${VELNOR_TOOL_RUST-}\""),
];

/// With keys a script job sets from a step output, because velnor-gen reads neither `github.run_attempt`
/// nor `runner.temp` in `with`, and one key that only narrows a checkout.
const IGNORED_WITH: [(&str, &str); 4] = [
    ("actions/checkout", "sparse-checkout"),
    ("actions/upload-artifact", "name"),
    ("actions/upload-artifact", "path"),
    ("actions/download-artifact", "path"),
];

/// The one step that gets an `id` the engine did not give it: the download step reads its directory
/// from this step's output.
const ID_FOR_A_STEP_OUTPUT: &str = "Validate exact artifact ID";

/// Step `env` keys of the engine that a script reads from its own context instead.
const MOVED_ENV: [&str; 4] = [
    "PATH",
    "VELNOR_RUST_VERSION",
    "SOURCES_METADATA",
    "SOURCES_DOWNLOADED",
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
    if condition(engine) != condition(generated) {
        unexpected.push(format!(
            "{id}: if: engine {:?}, generated {:?}",
            condition(engine),
            condition(generated)
        ));
    }
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
    if SCRIPTED.contains(&id) {
        note("a script replaces inline commands");
        scripted_steps(id, engine, generated, unexpected);
        return;
    }
    let (chains, flat) = engine_steps(engine);
    let (flat, piped) = replace_pipes(flat);
    if flat != gen_steps(generated) {
        unexpected.push(format!(
            "{id}: steps: engine {flat:#?}, generated {:#?}",
            gen_steps(generated)
        ));
    }
    if piped {
        note("a pipe is written as two steps");
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

fn steps_of(job: &Value) -> Vec<&Value> {
    job.get("steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect()
}

fn step_name(step: &Value) -> String {
    step.get("name").map(text).unwrap_or_default()
}

/// `bash qualification/<file> <args>` as a (file, args) pair.
fn script_call(run: &str) -> Option<(&str, &str)> {
    let rest = run.strip_prefix("bash qualification/")?;
    Some(rest.split_once(' ').unwrap_or((rest, "")))
}

/// What the engine passed to `bash -c` inside single quotes, with the splices undone.
fn engine_body(run: &str) -> Option<String> {
    let start = run.find("-c '")? + 4;
    let mut body = run[start..].strip_suffix('\'')?.replace("'\\''", "'");
    for (from, to) in SPLICES {
        body = body.replace(from, to);
    }
    Some(body)
}

fn scripted_steps(id: &str, engine: &Value, generated: &Value, unexpected: &mut Vec<String>) {
    let from_engine = steps_of(engine);
    let from_generated = steps_of(generated);
    let mut last = 0;
    for step in &from_generated {
        let name = step_name(step);
        let Some(position) = from_engine.iter().position(|seen| step_name(seen) == name) else {
            let checkout = step
                .get("uses")
                .map(text)
                .is_some_and(|uses| uses.starts_with("actions/checkout@"));
            if !(checkout && id.starts_with("ports-")) {
                unexpected.push(format!("{id}: {name}: the engine has no such step"));
            }
            continue;
        };
        if position < last {
            unexpected.push(format!("{id}: {name}: the steps are in another order"));
        }
        last = position;
        scripted_step(id, &name, from_engine[position], step, unexpected);
    }
    for step in &from_engine {
        let name = step_name(step);
        if !from_generated.iter().any(|seen| step_name(seen) == name) {
            unexpected.push(format!("{id}: {name}: the engine step is not generated"));
        }
    }
}

fn scripted_step(
    id: &str,
    name: &str,
    engine: &Value,
    generated: &Value,
    unexpected: &mut Vec<String>,
) {
    let mut differ = |what: &str| unexpected.push(format!("{id}: {name}: {what}"));
    let named_by_the_script = name == ID_FOR_A_STEP_OUTPUT && engine.get("id").is_none();
    if engine.get("id") != generated.get("id") && !named_by_the_script {
        differ("id");
    }
    let uses = engine.get("uses").map(text);
    if uses != generated.get("uses").map(text) {
        differ("uses");
    }
    let action = uses
        .as_deref()
        .and_then(|uses| uses.split('@').next())
        .unwrap_or_default();
    let with = |step: &Value| {
        table(step.get("with"))
            .into_iter()
            .filter(|(key, _)| !IGNORED_WITH.contains(&(action, key.as_str())))
            .collect::<BTreeMap<_, _>>()
    };
    if with(engine) != with(generated) {
        differ("with");
    }
    let engine_env = table(engine.get("env"));
    let generated_env = table(generated.get("env"));
    let engine_run = engine.get("run").map(text);
    let generated_run = generated.get("run").map(text);
    let Some((file, args)) = generated_run.as_deref().and_then(script_call) else {
        if engine_run != generated_run || engine_env != generated_env {
            differ("an inline step differs");
        }
        return;
    };
    if generated_env
        .iter()
        .any(|(key, value)| engine_env.get(key) != Some(value))
    {
        differ("env the engine does not set");
    }
    if engine_env.iter().any(|(key, value)| {
        !generated_env.contains_key(key) && !value.is_empty() && !MOVED_ENV.contains(&key.as_str())
    }) {
        differ("env the script does not get");
    }
    let path = format!("qualification/{file}");
    let Ok(script) = read(&path) else {
        differ("the script does not exist");
        return;
    };
    if !args.is_empty() && !script.contains(&format!("  {args})")) {
        differ("the script has no such phase");
    }
    let keeps = SCRIPT_KEEPS
        .iter()
        .find(|(kept, _)| *kept == path)
        .map_or(&[][..], |(_, fragments)| *fragments);
    if let Some(missing) = keeps.iter().find(|fragment| !script.contains(**fragment)) {
        differ(&format!("the script lost {missing}"));
    }
    verbatim_body(name, engine_run.as_deref().unwrap_or_default(), &mut differ);
}

/// The two engine verifiers moved into their own files unchanged, and the Mise pin check line by line.
fn verbatim_body(name: &str, run: &str, differ: &mut impl FnMut(&str)) {
    let file = match name {
        "Resolve project tool versions" => "qualification/stable-sources-resolve-tools.sh",
        "Verify Rust channel manifest" => "qualification/stable-sources-verify-rust.sh",
        "Verify selected ref, version, and binary digest" => "qualification/mise-pin.sh",
        _ => return,
    };
    let Ok(script) = read(file) else {
        differ("the script does not exist");
        return;
    };
    if file.ends_with("mise-pin.sh") {
        let lost = run.lines().find(|line| {
            !line.starts_with("case ") && !line.starts_with("set ") && !script.contains(*line)
        });
        if let Some(line) = lost {
            differ(&format!("the script lost the line {line}"));
        }
    } else if engine_body(run).is_none_or(|body| !script.contains(&body)) {
        differ("the script is not the engine's text");
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
    assert_eq!(listed("a script replaces inline commands"), set(&SCRIPTED));
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
