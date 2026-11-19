// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::Value;

use super::{read, table, text};

/// Jobs whose inline scripts are calls of a checked-in script under `qualification/`: velnor-gen
/// refuses a multi-line `run`, a loop, a pipe, a `shell` key and a job `env`.
pub(super) const SCRIPTED: [&str; 8] = [
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
        &[
            "binary=\"$(command -v mise)\"",
            "test \"$actual\" = \"$MISE_SHA256\"",
        ],
    ),
];

/// The engine splices the Rust version into the verifier text from the outer shell; the script reads it.
const SPLICES: [(&str, &str); 2] = [
    (
        "'\"${VELNOR_TOOL_RUST+VELNOR_TOOL_RUST=\"$VELNOR_TOOL_RUST\"}\"'",
        "${VELNOR_TOOL_RUST+VELNOR_TOOL_RUST=\"$VELNOR_TOOL_RUST\"}",
    ),
    (
        "v=\"'\"${VELNOR_TOOL_RUST}\"'\"",
        "v=\"${VELNOR_TOOL_RUST-}\"",
    ),
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

pub(super) fn scripted_steps(
    id: &str,
    engine: &Value,
    generated: &Value,
    unexpected: &mut Vec<String>,
) {
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
    // The image is whatever the engine runs now, so a bump of the engine's digest fails here.
    let image = engine_run.as_deref().and_then(|run| {
        run.split_whitespace()
            .find(|word| word.starts_with("alpine:"))
    });
    if image.is_some_and(|image| !script.contains(image)) {
        differ("the script runs another image than the engine");
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
