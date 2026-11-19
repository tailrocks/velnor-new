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

const OR_REPLACED: [(&str, &str); 1] = [(
    "test -z \"$HIT\" || test \"$HIT\" = false",
    "test \"$HIT\" != true",
)];

const NEWLINE_REPLACED: [(&str, &str); 1] = [(
    "printf '%s\n' cache-ok > \"g4 cache/note.txt\"",
    "printf '%s\\n' cache-ok > \"g4 cache/note.txt\"",
)];

const RESTORE_ONLY: (&str, &str, &str) = (
    "cache-hosted",
    "actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
    "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
);

const MESSAGE_REPLACED: [(&str, &str); 1] = [(
    "echo '::error title=cache_qualification_requires_non_default_branch::Dispatch cache qualification modes from a non-default branch; every cache job is skipped on the default branch.'",
    "echo '::error title=cache_qualification_requires_non_default_branch::Dispatch cache qualification modes from a non-default branch. Every cache job is skipped on the default branch.'",
)];

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

fn unparenthesised(condition: &str) -> Option<String> {
    let (mode, gate) = condition
        .strip_prefix('(')?
        .strip_suffix(')')?
        .split_once(") && (")?;
    Some(format!("{mode} && {gate}"))
}

fn restore_only(id: &str, steps: Vec<StepSignature>) -> (Vec<StepSignature>, bool) {
    let (job, from, to) = RESTORE_ONLY;
    let mut restored = false;
    let steps = steps
        .into_iter()
        .map(|step| {
            if id == job && step.uses.as_deref() == Some(from) {
                restored = true;
                StepSignature {
                    uses: Some(to.to_owned()),
                    ..step
                }
            } else {
                step
            }
        })
        .collect();
    (steps, restored)
}

fn replace_runs(steps: Vec<StepSignature>, table: &[(&str, &str)]) -> (Vec<StepSignature>, bool) {
    let mut replaced = false;
    let steps = steps
        .into_iter()
        .map(|step| {
            let hit = table
                .iter()
                .find(|(from, _)| step.run.as_deref() == Some(from));
            match hit {
                Some((_, to)) => {
                    replaced = true;
                    StepSignature {
                        run: Some((*to).to_owned()),
                        ..step
                    }
                }
                None => step,
            }
        })
        .collect();
    (steps, replaced)
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

type Kinds = BTreeMap<&'static str, BTreeSet<String>>;

struct Pair<'a> {
    id: &'a str,
    engine: &'a Value,
    engine_workflow: &'a Value,
    generated: &'a Value,
    generated_workflow: &'a Value,
}

fn compare_head(pair: &Pair<'_>, kinds: &mut Kinds, unexpected: &mut Vec<String>) {
    let Pair {
        id,
        engine,
        engine_workflow,
        generated,
        generated_workflow,
    } = *pair;
    let mut note = |kind: &'static str| {
        kinds.entry(kind).or_default().insert(id.to_owned());
    };
    let (unrolled, looped) = shapes::unroll(engine);
    let engine = &unrolled;
    if looped {
        note("a retry loop is written as plain probes");
    }
    shapes::compare(id, engine, generated, &mut note, unexpected);
    for key in ["name", "timeout-minutes", "needs", "outputs", "env"] {
        if engine.get(key) != generated.get(key) {
            unexpected.push(format!(
                "{id}: {key}: engine {:?}, generated {:?}",
                engine.get(key),
                generated.get(key)
            ));
        }
    }
    let engine_mode = engine.get("cache-mode");
    let inherited = engine_mode.or_else(|| engine_workflow.get("cache-mode"));
    let mode_differs = match generated.get("cache-mode") {
        Some(mode) => Some(mode) != inherited,
        None => engine_mode.is_some(),
    };
    if mode_differs {
        unexpected.push(format!(
            "{id}: cache-mode: engine {inherited:?}, generated {:?}",
            generated.get("cache-mode")
        ));
    }
    let generated_condition = condition(generated);
    if condition(engine) != generated_condition {
        if condition(engine) == generated_condition.as_deref().and_then(unparenthesised) {
            note("a case condition is parenthesised");
        } else {
            unexpected.push(format!(
                "{id}: if: engine {:?}, generated {generated_condition:?}",
                condition(engine)
            ));
        }
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
}

fn compare_steps(pair: &Pair<'_>, kinds: &mut Kinds, unexpected: &mut Vec<String>) {
    let Pair {
        id,
        engine,
        generated,
        ..
    } = *pair;
    let mut note = |kind: &'static str| {
        kinds.entry(kind).or_default().insert(id.to_owned());
    };
    let (unrolled, _) = shapes::unroll(engine);
    let (chains, flat) = engine_steps(&unrolled);
    let (flat, piped) = replace_pipes(flat);
    let (flat, ored) = replace_runs(flat, &OR_REPLACED);
    let (flat, worded) = replace_runs(flat, &MESSAGE_REPLACED);
    let (flat, escaped) = replace_runs(flat, &NEWLINE_REPLACED);
    let (flat, restored) = restore_only(id, flat);
    if flat != gen_steps(generated) {
        unexpected.push(format!(
            "{id}: steps: engine {flat:#?}, generated {:#?}",
            gen_steps(generated)
        ));
    }
    for (kind, happened) in [
        ("a pipe is written as two steps", piped),
        ("an || is written as one test", ored),
        ("a semicolon in a message is a full stop", worded),
        ("a line feed in a printf format is written as \\n", escaped),
        (
            "a restore-only actions/cache is actions/cache/restore",
            restored,
        ),
    ] {
        if happened {
            note(kind);
        }
    }
    if chains.len()
        != unrolled
            .get("steps")
            .map_or(0, |steps| steps.as_array().map_or(0, Vec::len))
        || flat.len() != chains.len()
    {
        note("an && chain is written as steps");
    }
}

const SCRIPTED: [&str; 2] = ["mbx-cache-write-hosted", "mbx-cache-read-hosted"];
const SCRIPT: &str = "qualification/mbx-cache-roundtrip.sh";
const IDENTITY_STEP: &str = "Verify MBX action identity and write policy";

const PHASES: [(&str, &str); 9] = [
    ("Resolve project tool versions", "resolve"),
    ("Install pinned Rust toolchain", "install-rust"),
    ("Verify Rust channel manifest", "verify-rust"),
    ("Put the verified Rust toolchain on PATH", "rust-path"),
    ("Require imported MBX objects", "require-imported"),
    ("Compile MBX cache probe", "probe"),
    ("Require reused compilation", "require-reuse"),
    ("Clean MBX workspace outputs", "clean"),
    ("Sample runner disk after cleanup", "disk"),
];

const SPLICED_ENV: &str = "'\"${VELNOR_TOOL_RUST+VELNOR_TOOL_RUST=\"$VELNOR_TOOL_RUST\"}\"'";
const SPLICED_VERSION: &str = "'\"${VELNOR_TOOL_RUST}\"'";

struct Pins {
    mbx: String,
    rust: String,
}

fn pinned(mise: &str, tool: &str) -> Result<String, Box<dyn Error>> {
    let prefix = format!("{tool} = {{ version = \"");
    let line = mise
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .ok_or_else(|| format!("mise.toml pins no {tool}"))?;
    Ok(line.split('"').next().unwrap_or_default().to_owned())
}

fn resolved(value: &str, pins: &Pins) -> String {
    value
        .replace("${{ env.VELNOR_TOOL_MR_BOXINGTON }}", &pins.mbx)
        .replace("${{ env.VELNOR_TOOL_RUST }}", &pins.rust)
}

fn engine_body(run: &str) -> (Option<&str>, String) {
    match run
        .starts_with("function_env_unset=()")
        .then(|| run.split_once(" -c '"))
        .flatten()
    {
        Some((wrapper, rest)) => (
            Some(wrapper),
            rest.strip_suffix('\'')
                .unwrap_or(rest)
                .replace("'\\''", "'")
                .replace(
                    SPLICED_ENV,
                    "${VELNOR_TOOL_RUST+VELNOR_TOOL_RUST=\"$VELNOR_TOOL_RUST\"}",
                )
                .replace(SPLICED_VERSION, "${VELNOR_TOOL_RUST}"),
        ),
        None => (None, run.to_owned()),
    }
}

fn steps_of(job: &Value) -> &[Value] {
    job.get("steps")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn scripted_run(
    id: &str,
    name: &str,
    engine_run: &str,
    generated_run: Option<&str>,
    script: &str,
    unexpected: &mut Vec<String>,
) {
    let phase = if name == IDENTITY_STEP {
        Some(if id.contains("write") {
            "identity-write"
        } else {
            "identity-read"
        })
    } else {
        PHASES
            .iter()
            .find(|(step, _)| *step == name)
            .map(|(_, phase)| *phase)
    };
    let (wrapper, body) = engine_body(engine_run);
    let expected = phase.map(|phase| format!("bash {SCRIPT} {phase}"));
    if expected.is_none() || generated_run != expected.as_deref() {
        unexpected.push(format!(
            "{id}: {name}: run {generated_run:?}, expected {expected:?}"
        ));
    }
    if !script.contains(&body) || wrapper.is_some_and(|wrapper| !script.contains(wrapper)) {
        unexpected.push(format!(
            "{id}: {name}: the script does not hold the engine run verbatim"
        ));
    }
}

fn compare_scripted(pair: &Pair<'_>, script: &str, pins: &Pins, unexpected: &mut Vec<String>) {
    let id = pair.id;
    let (engine, generated) = (steps_of(pair.engine), steps_of(pair.generated));
    if engine.len() != generated.len() {
        unexpected.push(format!(
            "{id}: {} engine steps, {} generated",
            engine.len(),
            generated.len()
        ));
        return;
    }
    for (engine, generated) in engine.iter().zip(generated) {
        let name = engine.get("name").map(text).unwrap_or_default();
        for (key, same) in [
            ("name", engine.get("name") == generated.get("name")),
            ("id", engine.get("id") == generated.get("id")),
            ("uses", engine.get("uses") == generated.get("uses")),
            (
                "env",
                table(engine.get("env")) == table(generated.get("env")),
            ),
            (
                "if",
                engine.get("if").map(expression) == generated.get("if").map(expression),
            ),
            (
                "with",
                table(engine.get("with"))
                    .into_iter()
                    .map(|(k, v)| (k, resolved(&v, pins)))
                    .collect::<BTreeMap<_, _>>()
                    == table(generated.get("with")),
            ),
        ] {
            if !same {
                unexpected.push(format!(
                    "{id}: {name}: {key}: engine {engine:?}, generated {generated:?}"
                ));
            }
        }
        match engine.get("run").map(text) {
            Some(run) => scripted_run(
                id,
                &name,
                &run,
                generated.get("run").and_then(Value::as_str),
                script,
                unexpected,
            ),
            None if generated.get("run").is_some() => {
                unexpected.push(format!("{id}: {name}: a run where the engine has a uses"));
            }
            None => {}
        }
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
