// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use serde_json::Value;

use super::compare::Pair;
use super::{expression, table, text};

pub(super) const SCRIPTED: [&str; 2] = ["mbx-cache-write-hosted", "mbx-cache-read-hosted"];
pub(super) const SCRIPT: &str = "qualification/mbx-cache-roundtrip.sh";
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

pub(super) struct Pins {
    pub(super) mbx: String,
    pub(super) rust: String,
}

pub(super) fn pinned(mise: &str, tool: &str) -> Result<String, Box<dyn Error>> {
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

pub(super) fn compare_scripted(
    pair: &Pair<'_>,
    script: &str,
    pins: &Pins,
    unexpected: &mut Vec<String>,
) {
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
