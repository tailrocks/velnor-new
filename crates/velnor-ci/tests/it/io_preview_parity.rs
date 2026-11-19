// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `verify-runtime` against the legacy `source_preview_runtime.py`, on one recorded
//! GitHub: the same verdict, the same API requests and the same installed files.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde_json::{Value, json};
use velnor_ci::io::preview::gh::GhAttestor;
use velnor_ci::io::preview::install::install;
use velnor_ci::preview::runtime::{Host, verify_runtime};

use crate::preview_fakes::FakeNet;
use crate::preview_mutations::MUTATIONS;
use crate::preview_world::{PUBLIC, PUBLIC_RUN, World, sha};
use crate::support::fresh_dir;

fn legacy() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../velnor-actions-mise/src/source_preview")
}

fn harness() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/data/preview_parity/verify_runtime.py")
}

fn python_ready() -> bool {
    Command::new("python3")
        .args(["-I", "-S", "-c", "import tomllib"])
        .output()
        .is_ok_and(|out| out.status.success())
}

fn canonical_dir(name: &str) -> PathBuf {
    let dir = fresh_dir("velnor-ci-preview", name);
    fs::canonicalize(dir).unwrap_or_else(|e| unreachable!("{e}"))
}

fn fake_gh(dir: &Path, attestation: &Value) -> (PathBuf, String) {
    let script = format!("#!/bin/sh\ncat <<'JSON'\n{attestation}\nJSON\n");
    let path = dir.join("gh");
    fs::write(&path, &script).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    (path, sha(script.as_bytes()))
}

fn world_json(world: &World) -> Value {
    let assets: BTreeMap<&String, String> = world
        .assets
        .iter()
        .map(|(url, body)| (url, velnor_ci::preview::json::hex(body)))
        .collect();
    json!({"api": world.api, "assets": assets, "public_source": PUBLIC, "public_run": PUBLIC_RUN})
}

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(root)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .flatten()
    {
        let body = fs::read(entry.path()).unwrap_or_else(|e| unreachable!("{e}"));
        files.insert(entry.file_name().to_string_lossy().into_owned(), body);
    }
    files
}

struct Outcome {
    ok: bool,
    calls: Vec<String>,
    outputs: String,
    files: BTreeMap<String, Vec<u8>>,
}

fn legacy_outcome(world: &World, name: &str) -> Outcome {
    let dir = canonical_dir(&format!("py-{name}"));
    let temp = dir.join("temp");
    fs::create_dir(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    let (gh, digest) = fake_gh(&dir, &world.attestation);
    let file = dir.join("world.json");
    fs::write(&file, world_json(world).to_string()).unwrap_or_else(|e| unreachable!("{e}"));
    let out = Command::new("python3")
        .args(["-I", "-S"])
        .arg(harness())
        .arg(legacy())
        .arg(&file)
        .arg(&temp)
        .arg(&gh)
        .arg(&digest)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = String::from_utf8_lossy(&out.stdout);
    let value: Value = serde_json::from_str(text.trim())
        .unwrap_or_else(|e| unreachable!("{e}: {text} {}", String::from_utf8_lossy(&out.stderr)));
    let ok = value["ok"].as_bool() == Some(true);
    let calls = value["calls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c.as_str().map(str::to_owned))
        .collect();
    let files = if ok {
        tree(&temp.join("source-preview-runtime"))
    } else {
        BTreeMap::new()
    };
    Outcome {
        ok,
        calls,
        outputs: value["outputs"].as_str().unwrap_or("").to_owned(),
        files,
    }
}

fn helper_outcome(world: &World, name: &str) -> Outcome {
    let dir = canonical_dir(&format!("rs-{name}"));
    let temp = dir.join("temp");
    fs::create_dir(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    let (gh, digest) = fake_gh(&dir, &world.attestation);
    let attestor = GhAttestor::with_hashes(gh, temp.clone(), BTreeSet::from([digest]));
    let net = FakeNet::new(world.clone());
    let verified = verify_runtime(&net, &attestor, Host::Linux, PUBLIC, PUBLIC_RUN, 1);
    let calls = net.calls.borrow().clone();
    let Ok(verified) = verified else {
        return Outcome {
            ok: false,
            calls,
            outputs: String::new(),
            files: BTreeMap::new(),
        };
    };
    let outputs = verified.outputs();
    let installed = install(
        &temp,
        &verified,
        Instant::now() + std::time::Duration::from_secs(60),
        || Ok(()),
    );
    let files = installed.map(|path| tree(&path)).unwrap_or_default();
    Outcome {
        ok: !files.is_empty(),
        calls,
        outputs,
        files,
    }
}

#[test]
fn preview_parity_runtime_happy_path_matches_the_legacy_script() {
    if !python_ready() {
        eprintln!("skipped: python3 with tomllib is not available");
        return;
    }
    let world = World::new();
    let (old, new) = (legacy_outcome(&world, "ok"), helper_outcome(&world, "ok"));
    assert!(old.ok && new.ok);
    assert_eq!(old.calls, new.calls);
    assert_eq!(old.outputs, new.outputs);
    assert_eq!(
        old.files.keys().collect::<Vec<_>>(),
        [
            "bundle.json",
            "darwin",
            "identity.json",
            "linux",
            "receipt.json"
        ]
    );
    assert_eq!(old.files, new.files);
}

#[test]
fn preview_parity_runtime_refusals_match_the_legacy_script() {
    if !python_ready() {
        eprintln!("skipped: python3 with tomllib is not available");
        return;
    }
    for (index, (name, mutate)) in MUTATIONS.iter().enumerate() {
        let mut world = World::new();
        mutate(&mut world);
        let old = legacy_outcome(&world, &format!("m{index}"));
        let new = helper_outcome(&world, &format!("m{index}"));
        assert!(!old.ok, "the legacy script accepted: {name}");
        assert!(!new.ok, "the helper accepted: {name}");
    }
}
