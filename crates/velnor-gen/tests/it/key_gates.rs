// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Every config key the `velnor-ci` parsers accept has a row in the helper-gate table, so a new
//! key cannot land without naming the first release that reads it.
//!
//! The accepted keys come from the parsers themselves: a struct that denies unknown fields lists
//! its fields in the error for a field it does not have, so the walk below probes each key with a
//! table and with an array of tables and reads the field list out of that refusal.

use std::collections::BTreeSet;
use std::error::Error;

use velnor_gen::{ConfigFile, GenerateError, key_gates};
use velnor_model::Selection;
use velnor_release::{ReleaseFiles, ReleasePlan};
use velnor_rust::{BUDGET_KEYS, KEYS, detect};

use crate::pipeline::{pin_helper, view, workflow};

type Probe = dyn Fn(&toml::Table) -> String;

const PROBE: &str = "__probe__";

/// The refusal text for `table`, from the `rust-ci.toml` parser.
fn rust_ci_probe(table: &toml::Table) -> String {
    let bytes = table.to_string().into_bytes();
    match view(&["Cargo.toml"], Some(&bytes)) {
        Ok(repo) => format!("{:?}", detect(&repo).err()),
        Err(error) => error.to_string(),
    }
}

/// The refusal text for `table`, from the `release.toml` parser.
fn release_probe(table: &toml::Table) -> String {
    let files = ReleaseFiles {
        release_toml: Some(table.to_string()),
        ..ReleaseFiles::default()
    };
    format!("{:?}", ReleasePlan::read(&files).err())
}

/// `table` nested under `path`, where `true` wraps the level in an array of tables.
fn nested(path: &[(String, bool)], leaf: toml::Value) -> toml::Table {
    let mut inner = leaf;
    for (name, array) in path.iter().rev() {
        let value = if *array {
            toml::Value::Array(vec![inner])
        } else {
            inner
        };
        inner = toml::Value::Table(toml::Table::from_iter([(name.clone(), value)]));
    }
    match inner {
        toml::Value::Table(table) => table,
        _ => toml::Table::new(),
    }
}

fn probe_table() -> toml::Value {
    toml::Value::Table(toml::Table::from_iter([(
        PROBE.to_owned(),
        toml::Value::Integer(1),
    )]))
}

/// The field list a struct under `path` gives when it refuses the probe field, or `None` when
/// the value at `path` is not a struct.
fn fields(probe: &Probe, path: &[(String, bool)]) -> Option<Vec<String>> {
    let text = probe(&nested(path, probe_table()));
    let marker = format!("unknown field `{PROBE}`, expected ");
    let rest = &text[text.find(&marker)? + marker.len()..];
    let line = rest.split(['"', '\\']).next()?;
    Some(
        line.split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect(),
    )
}

/// Adds every key under `path` to `found`, as dotted paths.
fn walk(probe: &Probe, path: &[(String, bool)], names: &[String], found: &mut BTreeSet<String>) {
    for name in names {
        let mut here = path.to_vec();
        here.push((name.clone(), false));
        let mut as_array = path.to_vec();
        as_array.push((name.clone(), true));
        let dotted = here
            .iter()
            .map(|(part, _)| part.as_str())
            .collect::<Vec<_>>()
            .join(".");
        found.insert(dotted);
        for candidate in [&here, &as_array] {
            if let Some(children) = fields(probe, candidate) {
                walk(probe, candidate, &children, found);
                break;
            }
        }
    }
}

fn rust_ci_keys() -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let names: Vec<String> = KEYS
        .iter()
        .filter(|key| **key != "budget")
        .map(ToString::to_string)
        .collect();
    walk(&rust_ci_probe, &[], &names, &mut found);
    // `[budget]` checks its keys by hand rather than through serde, so read its own lists.
    found.insert("budget".to_owned());
    for key in BUDGET_KEYS {
        found.insert(format!("budget.{key}"));
    }
    for mode in Selection::ALL {
        found.insert(format!("budget.{}", mode.name()));
        for key in BUDGET_KEYS {
            found.insert(format!("budget.{}.{key}", mode.name()));
        }
    }
    found
}

fn release_keys() -> Result<BTreeSet<String>, Box<dyn Error>> {
    let mut found = BTreeSet::new();
    let names = fields(&release_probe, &[]).ok_or("release.toml does not deny unknown keys")?;
    walk(&release_probe, &[], &names, &mut found);
    Ok(found)
}

fn rows(file: ConfigFile) -> BTreeSet<String> {
    key_gates()
        .filter(|row| row.file == file)
        .map(|row| row.key.to_owned())
        .collect()
}

#[test]
fn every_key_the_rust_ci_parser_accepts_has_a_helper_gate_row() {
    let missing: Vec<_> = rust_ci_keys()
        .difference(&rows(ConfigFile::RustCi))
        .cloned()
        .collect();
    assert!(
        missing.is_empty(),
        "add a row to crates/velnor-gen/src/key_gates/rust_ci.rs for {missing:?}"
    );
}

#[test]
fn every_key_the_release_parser_accepts_has_a_helper_gate_row() -> Result<(), Box<dyn Error>> {
    let missing: Vec<_> = release_keys()?
        .difference(&rows(ConfigFile::Release))
        .cloned()
        .collect();
    assert!(
        missing.is_empty(),
        "add a row to crates/velnor-gen/src/key_gates/release.rs for {missing:?}"
    );
    Ok(())
}

#[test]
fn no_row_names_a_key_the_parsers_dropped() -> Result<(), Box<dyn Error>> {
    let release = release_keys()?;
    let stale: Vec<_> = rows(ConfigFile::RustCi)
        .difference(&rust_ci_keys())
        .chain(rows(ConfigFile::Release).difference(&release))
        .cloned()
        .collect();
    assert!(stale.is_empty(), "remove the rows for {stale:?}");
    Ok(())
}

/// The error for `config` under a pin of `version`.
fn refusal(config: &str, version: &str) -> Result<Option<GenerateError>, Box<dyn Error>> {
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    pin_helper(&mut repo, version)?;
    Ok(workflow(&repo)
        .err()
        .and_then(|error| error.downcast::<GenerateError>().ok())
        .map(|error| *error))
}

fn names_key(error: Option<GenerateError>, wanted: &str, first: &str) -> bool {
    matches!(&error, Some(GenerateError::ConfigKeyHelperTooOld { key, .. }) if *key == wanted)
        && error.is_some_and(|error| error.to_string().contains(&format!("pin-helper {first}")))
}

#[test]
fn a_written_key_is_refused_one_release_below_its_row() -> Result<(), Box<dyn Error>> {
    for (config, key, first, below) in [
        (
            "sparse_checkout = false\n",
            "sparse_checkout",
            "0.1.92",
            "0.1.91",
        ),
        (
            "fork_runner = \"ubuntu-latest\"\n",
            "fork_runner",
            "0.1.82",
            "0.1.81",
        ),
        (
            "cache_budget_bytes = 1000000000\n",
            "cache_budget_bytes",
            "0.1.82",
            "0.1.81",
        ),
        (
            "[cache]\nmbx_remote = \"github-actions\"\n",
            "cache",
            "0.1.88",
            "0.1.87",
        ),
        (
            "mbx_shared_store = false\n",
            "mbx_shared_store",
            "0.1.85",
            "0.1.84",
        ),
        (
            "[ci]\nskip_push_after_merge_group = false\n",
            "ci",
            "0.1.88",
            "0.1.87",
        ),
        ("hosted = true\n", "hosted", "0.1.99", "0.1.98"),
    ] {
        let refused = refusal(config, below)?;
        assert!(
            names_key(refused.clone(), key, first),
            "{config} at {below}: {refused:?}"
        );
        let accepted = refusal(config, first)?;
        assert!(
            !matches!(accepted, Some(GenerateError::ConfigKeyHelperTooOld { .. })),
            "{config} at {first}: {accepted:?}"
        );
    }
    Ok(())
}

#[test]
fn the_newest_written_key_decides_the_version_the_error_names() -> Result<(), Box<dyn Error>> {
    let config = "sparse_checkout = true\nfork_runner = \"ubuntu-latest\"\n";
    let refused = refusal(config, "0.1.81")?;
    assert!(
        names_key(refused.clone(), "sparse_checkout", "0.1.92"),
        "{refused:?}"
    );
    Ok(())
}

/// Keys that the released tools-v0.1.107 velnor-ci parses (its `plan` accepts each one in a
/// minimal `.velnor/rust-ci.toml`), so a 0.1.107 pin must not be refused for them.
#[test]
fn the_keys_tools_v0_1_107_parses_are_accepted_at_107() -> Result<(), Box<dyn Error>> {
    for config in [
        "[[test_lane]]\nroot = \".\"\nall_features = true\n",
        "[[test_lane]]\nroot = \".\"\nno_default_features = true\n",
        "[[test_groups]]\nname = \"unit\"\nfilter = \"all()\"\nneeds = [\"cargo\"]\nrestore_cache = true\n",
        "[[test_groups]]\nname = \"unit\"\nfilter = \"all()\"\nno_tests = \"warn\"\n",
    ] {
        let refused = refusal(config, "0.1.107")?;
        assert!(
            !matches!(refused, Some(GenerateError::ConfigKeyHelperTooOld { .. })),
            "{config} at 0.1.107: {refused:?}"
        );
    }
    Ok(())
}
