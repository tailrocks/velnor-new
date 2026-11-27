// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::time::Instant;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::exec_support::Scratch;
use crate::native_cache_support::{Run, Store, command, patched, ran_of};

const SEED: &str = "touch ran && mkdir -p data && printf hello > data/a.txt";

fn run_with(edit: impl FnOnce(&mut Value)) -> Result<(Run, Store)> {
    let store = Store::new()?;
    let work = Scratch::new()?;
    let job = patched("cache-save", &work, &[("script", SEED)])?;
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&job)?)?;
    let step = value["Steps"]
        .as_array_mut()
        .and_then(|steps| steps.iter_mut().find(|step| step["Name"] == "cache"))
        .context("the cache step")?;
    edit(&mut step["Inputs"]);
    fs::write(&job, serde_json::to_string(&value)?)?;
    let started = Instant::now();
    let output = command(&job, &work, &store).output()?;
    Ok((
        Run {
            ran: ran_of(&output, started),
            work,
        },
        store,
    ))
}

fn expression(source: &str) -> Value {
    json!({"type": 3, "expr": source})
}

fn refused_when_the_step_runs(run: &Run, store: &Store) -> Result<()> {
    assert_eq!(run.ran.code, Some(1), "{}", run.ran.stdout);
    let stdout = run.ran.stdout.to_ascii_lowercase();
    assert!(
        stdout.contains(
            "unsupported: actions/cache input 'enablecrossosarchive' is not supported by velnor-runner"
        ),
        "{}",
        run.ran.stdout
    );
    assert!(
        run.workspace_file("ran").exists(),
        "the earlier step did not run"
    );
    assert!(store.entries()?.is_empty());
    Ok(())
}

#[test]
fn an_expression_with_enable_cross_os_archive_is_refused_when_the_step_runs() -> Result<()> {
    let (run, store) = run_with(|inputs| {
        *inputs = expression(
            r#"fromJSON('{"path":"data","key":"k-expr","enableCrossOsArchive":"true"}')"#,
        );
    })?;
    refused_when_the_step_runs(&run, &store)
}

#[test]
fn an_expression_input_name_is_refused_when_the_step_runs() -> Result<()> {
    let (run, store) = run_with(|inputs| {
        let map = inputs["map"].as_array_mut().expect("the input map");
        map.push(json!({
            "key": expression("'enableCrossOsArchive'"),
            "value": {"type": 0, "lit": "true"},
        }));
    })?;
    refused_when_the_step_runs(&run, &store)
}

#[test]
fn an_expression_with_supported_inputs_saves() -> Result<()> {
    let (run, store) = run_with(|inputs| {
        *inputs = expression(r#"fromJSON('{"path":"data","key":"k-expr"}')"#);
    })?;
    assert!(run.succeeded(), "{}", run.ran.stdout);
    assert!(run.ran.has_line("Cache saved with key: k-expr"));
    assert_eq!(store.entries()?.len(), 1);
    Ok(())
}
