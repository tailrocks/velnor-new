// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use velnor_ci::preview::authority::modes::Mode;

use crate::preview_authority_world::*;
use crate::preview_world::R;

fn python_ready() -> bool {
    std::process::Command::new("python3")
        .args(["-I", "-S", "-c", "import json"])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Run the legacy script for `mode` in the scenario's directory, with its environment.
fn legacy(s: &Scenario, mode: &str) -> Value {
    let renderer = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../crates/velnor-actions-workflow-renderer/src");
    let harness =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/data/preview_parity/run_authority.py");
    let world = s.base.join("../world.json");
    fs::write(&world, json!({"api": *s.client.api.borrow()}).to_string())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut command = std::process::Command::new("python3");
    command
        .args(["-I", "-S"])
        .arg(harness)
        .arg(renderer)
        .arg(&world)
        .current_dir(&s.base)
        .env_clear();
    for (key, value) in &s.vars {
        command.env(key, value);
    }
    let out = command
        .env("PREVIEW_MODE", mode)
        .env("GH_TOKEN", "x")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let value: Value = serde_json::from_str(text.trim())
        .unwrap_or_else(|e| unreachable!("{e}: {text} {}", String::from_utf8_lossy(&out.stderr)));
    assert_eq!(value["ok"], true, "the legacy script refused");
    value
}

#[test]
fn preview_parity_authority_draft_and_prepare_write_what_the_legacy_script_writes() {
    if !python_ready() {
        eprintln!("skipped: python3 is not available");
        return;
    }
    let artifacts = json!({"total_count": 0, "artifacts": []});
    let old = Scenario::new("parity-old-draft", 2);
    old.client.set(
        "/actions/runs/500/artifacts?per_page=100&page=1",
        artifacts.clone(),
    );
    let created = legacy(&old, "draft")["release"].clone();
    let new = Scenario::new("parity-new-draft", 2);
    new.client
        .set("/actions/runs/500/artifacts?per_page=100&page=1", artifacts);
    new.run(Mode::Draft).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(old.outputs(), new.outputs());
    let decision =
        |s: &Scenario| fs::read(s.temp.join("source-preview-decision.json")).unwrap_or_default();
    assert_eq!(decision(&old), decision(&new));
    assert!(decision(&new).len() > 2);
    assert_eq!(
        created,
        new.client.api.borrow()[&format!("{R}/releases/9000")]
    );
    for (label, scenario) in [("old", &old), ("new", &new)] {
        let target = scenario
            .base
            .join("source-preview-decision/source-preview-decision.json");
        write(&target, &decision(scenario));
        assert!(target.is_file(), "{label}");
    }
    for (scenario, python) in [(&old, true), (&new, false)] {
        let next = Scenario::new(
            if python {
                "parity-old-prepare"
            } else {
                "parity-new-prepare"
            },
            3,
        );
        fs::remove_dir_all(&next.base).unwrap_or_else(|e| unreachable!("{e}"));
        fs::rename(&scenario.base, &next.base).unwrap_or_else(|e| unreachable!("{e}"));
        let mut next = next;
        next.client = FakeClient {
            api: RefCell::new(new.client.api.borrow().clone()),
            zips: BTreeMap::new(),
            writes: RefCell::new(Vec::new()),
            deadline: scenario.client.deadline,
        };
        next.vars.insert("SIGN".to_owned(), "true".to_owned());
        if python {
            legacy(&next, "prepare");
        } else {
            next.run(Mode::Prepare)
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        let receipt = fs::read(next.temp.join("SDK_PREVIEW_RECEIPT.json")).unwrap_or_default();
        assert!(receipt.len() > 2);
        fs::write(scenario.temp.join("receipt.copy"), &receipt)
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(scenario.temp.join("prepare-outputs.copy"), next.outputs())
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let copy = |s: &Scenario, name: &str| fs::read(s.temp.join(name)).unwrap_or_default();
    assert_eq!(copy(&old, "receipt.copy"), copy(&new, "receipt.copy"));
    assert_eq!(
        copy(&old, "prepare-outputs.copy"),
        copy(&new, "prepare-outputs.copy")
    );
}
