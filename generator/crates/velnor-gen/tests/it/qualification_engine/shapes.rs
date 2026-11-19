// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

type Loops = [(&'static str, &'static [&'static str]); 3];

const LOOPS: Loops = [
    (
        "timeout 20 bash -c 'until echo >/dev/tcp/127.0.0.1/6379; do sleep 1; done'",
        &["nc -z -w 1 127.0.0.1 6379"],
    ),
    (
        "i=0; while [ \"$i\" -lt 20 ]; do nc -z -w 1 redis 6379 && exit 0; i=$((i+1)); sleep 1; done; exit 1",
        &["nc -z -w 1 redis 6379"],
    ),
    (
        "i=0; while [ \"$i\" -lt 30 ]; do nc -z -w 1 127.0.0.1 6379 && break; i=$((i+1)); sleep 1; done; nc -z -w 1 127.0.0.1 6379 && echo service-up && sleep 900",
        &["nc -z -w 1 127.0.0.1 6379", "echo service-up", "sleep 900"],
    ),
];

pub(super) fn unroll(job: &Value) -> (Value, bool) {
    let mut job = job.clone();
    let mut looped = false;
    if let Some(steps) = job.get_mut("steps").and_then(Value::as_array_mut) {
        for step in std::mem::take(steps) {
            let hit = step
                .get("run")
                .and_then(Value::as_str)
                .and_then(|run| LOOPS.iter().find(|(from, _)| *from == run));
            let Some((_, runs)) = hit else {
                steps.push(step);
                continue;
            };
            looped = true;
            for run in *runs {
                let mut one = step.clone();
                one["run"] = Value::String((*run).to_owned());
                steps.push(one);
            }
        }
    }
    (job, looped)
}

fn container_image(job: &Value) -> Option<String> {
    let container = job.get("container")?;
    container
        .as_str()
        .or_else(|| container.get("image").and_then(Value::as_str))
        .map(str::to_owned)
}

fn container_is_a_string(job: &Value) -> bool {
    job.get("container").is_some_and(Value::is_string)
}

fn has_step_shell(job: &Value) -> bool {
    job.get("steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|step| step.get("shell").is_some())
}

pub(super) fn compare(
    id: &str,
    engine: &Value,
    generated: &Value,
    note: &mut impl FnMut(&'static str),
    unexpected: &mut Vec<String>,
) {
    if container_image(engine) != container_image(generated) {
        unexpected.push(format!(
            "{id}: container: engine {:?}, generated {:?}",
            engine.get("container"),
            generated.get("container")
        ));
    }
    if container_is_a_string(engine) {
        note("a container is written as a mapping");
    }
    if engine.get("services") != generated.get("services") {
        unexpected.push(format!(
            "{id}: services: engine {:?}, generated {:?}",
            engine.get("services"),
            generated.get("services")
        ));
    }
    if has_step_shell(engine) {
        note("a step shell is not written");
    }
}

pub(super) fn assert_notes(
    kinds: &BTreeMap<&'static str, BTreeSet<String>>,
    scale_sets: &BTreeSet<String>,
) {
    let listed = |kind: &str| kinds.get(kind).cloned().unwrap_or_default();
    let set =
        |ids: &[&str]| -> BTreeSet<String> { ids.iter().map(|id| (*id).to_owned()).collect() };
    let containers = set(&["container-hosted", "container-scale-set"]);
    let mut shells = scale_sets.clone();
    shells.insert("container-hosted".to_owned());
    assert_eq!(listed("defaults.run.shell is not written"), shells);
    assert_eq!(listed("a step shell is not written"), containers);
    assert_eq!(listed("a container is written as a mapping"), containers);
    assert_eq!(
        listed("a retry loop is written as plain probes"),
        set(&[
            "services-hosted",
            "services-scale-set",
            "container-hosted",
            "container-scale-set",
            "cancel-service-hosted",
            "cancel-service-scale-set"
        ])
    );
}
