// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{condition, engine_steps, gen_steps, lane, permissions, replaced, shapes};

pub(super) type Kinds = BTreeMap<&'static str, BTreeSet<String>>;

pub(super) struct Pair<'a> {
    pub(super) id: &'a str,
    pub(super) engine: &'a Value,
    pub(super) engine_workflow: &'a Value,
    pub(super) generated: &'a Value,
    pub(super) generated_workflow: &'a Value,
}

pub(super) fn compare_head(pair: &Pair<'_>, kinds: &mut Kinds, unexpected: &mut Vec<String>) {
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
        if condition(engine)
            == generated_condition
                .as_deref()
                .and_then(replaced::unparenthesised)
        {
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

pub(super) fn compare_steps(pair: &Pair<'_>, kinds: &mut Kinds, unexpected: &mut Vec<String>) {
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
    let (flat, applied) = replaced::apply(id, flat);
    if flat != gen_steps(generated) {
        unexpected.push(format!(
            "{id}: steps: engine {flat:#?}, generated {:#?}",
            gen_steps(generated)
        ));
    }
    for kind in applied {
        note(kind);
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
