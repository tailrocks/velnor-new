// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::report::{LaneMetrics, check, waste};
use velnor_ci::waste_evidence::{mbx_seed_truncated, mbx_sessions, mbx_unparsed_lines};

fn metrics(value: Value) -> LaneMetrics {
    serde_json::from_value(value).unwrap_or_else(|error| unreachable!("{error}"))
}

fn quiet() -> Value {
    let mut value = super::report::lane();
    value["caches"] = json!([]);
    value["invocations"][0]["compilations"] = json!([]);
    value
}

fn cache(family: &str, hit: &str, entry_before_job: Option<bool>) -> Value {
    let mut value = json!({"family":family,"root":null,"restore_step":"restore","save_step":"save","requested_key":"velnor-src-key","matched_key":null,"hit":hit,"bytes":null,"usable":false,"usable_same_lock":false,"save":"not_attempted"});
    if let Some(known) = entry_before_job {
        value["entry_before_job"] = json!(known);
    }
    value
}

fn artifact(filename: &str) -> Value {
    json!({
        "package_id":"registry+https://example.invalid/index#serde@1.0.0",
        "manifest_path":"registry/serde/Cargo.toml",
        "target":{"name":"serde","kind":["lib"],"crate_types":["lib"],"src_path":"registry/serde/src/lib.rs"},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
        "features":["default"],"filenames":[filename],"fresh":false
    })
}

#[test]
fn sources_miss_is_waste_only_when_a_saved_entry_existed_before_the_job() {
    for (known, expected) in [(Some(true), true), (Some(false), false), (None, false)] {
        let mut value = quiet();
        value["caches"] = json!([cache("sources", "miss", known)]);
        let found = waste(&metrics(value));
        assert_eq!(
            found
                .iter()
                .any(|line| line.contains("sources cache missed velnor-src-key")),
            expected,
            "{known:?}: {found:?}"
        );
    }
    let mut value = quiet();
    value["caches"] = json!([cache("sources", "exact", Some(true))]);
    assert_eq!(waste(&metrics(value)), Vec::<String>::new());
}

#[test]
fn sources_miss_with_an_earlier_entry_fails_the_check() {
    let mut value = quiet();
    value["caches"] = json!([cache("sources", "miss", Some(true))]);
    let error = check(&[metrics(value)])
        .err()
        .map(|error| error.to_string());
    assert!(
        error.is_some_and(|text| text.contains("sources cache missed")),
        "the check must fail"
    );
}

fn mbx(primed: bool, sessions: &[(u64, u64, u64)]) -> Value {
    json!({"store_primed":primed,"sessions":sessions.iter().map(|(hits, misses, not_looked_up)| json!({"hits":hits,"misses":misses,"not_looked_up":not_looked_up})).collect::<Vec<_>>()})
}

fn path_artifact() -> Value {
    json!({
        "package_id":"path+file:///checkout/crates/mine#mine@0.1.0",
        "manifest_path":"crates/mine/Cargo.toml",
        "target":{"name":"mine","kind":["lib"],"crate_types":["lib"],"src_path":"crates/mine/src/lib.rs"},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
        "features":[],"filenames":["target/debug/deps/libmine-abc.rlib"],"fresh":false
    })
}

#[test]
fn an_empty_mbx_session_on_a_primed_store_is_waste() {
    for (evidence, expected) in [
        (mbx(true, &[(0, 0, 567)]), true),
        (mbx(true, &[(0, 12, 0)]), true),
        (mbx(true, &[(0, 0, 567), (40, 2, 0)]), false),
        (mbx(false, &[(0, 0, 567)]), false),
        (mbx(true, &[(0, 0, 0)]), false),
        (mbx(true, &[]), false),
    ] {
        let mut value = quiet();
        value["invocations"][0]["artifacts"] =
            json!([artifact("target/debug/deps/libserde-abc.rlib")]);
        value["mbx"] = evidence.clone();
        let found = waste(&metrics(value));
        assert_eq!(
            found.iter().any(|line| line.contains("MBX object cache")),
            expected,
            "{evidence}: {found:?}"
        );
    }
    assert_eq!(waste(&metrics(quiet())), Vec::<String>::new());
}

#[test]
fn an_empty_mbx_session_that_compiled_only_path_units_is_not_waste() {
    for (artifacts, expected) in [
        (json!([path_artifact()]), false),
        (
            json!([
                path_artifact(),
                artifact("target/debug/deps/libserde-abc.rlib")
            ]),
            true,
        ),
    ] {
        let mut value = quiet();
        value["invocations"][0]["artifacts"] = artifacts.clone();
        value["mbx"] = mbx(true, &[(0, 0, 567)]);
        let found = waste(&metrics(value));
        assert_eq!(
            found.iter().any(|line| line.contains("MBX object cache")),
            expected,
            "{artifacts}: {found:?}"
        );
    }
}

#[test]
fn a_toolchain_install_on_an_exact_tools_hit_is_waste() {
    for (hit, installs, expected) in [
        ("exact", json!(["component rust-src"]), true),
        ("exact", json!(["target wasm32-unknown-unknown"]), true),
        ("exact", json!(["toolchain 1.99.1"]), true),
        ("exact", json!([]), false),
        ("miss", json!(["component rust-src"]), false),
        ("fallback", json!(["component rust-src"]), false),
    ] {
        let mut value = quiet();
        value["caches"] = json!([cache("tools", hit, None)]);
        value["toolchain_installs"] = installs.clone();
        let found = waste(&metrics(value));
        assert_eq!(
            found
                .iter()
                .any(|line| line.contains("toolchain installed on an exact tools cache hit")),
            expected,
            "{hit} {installs}: {found:?}"
        );
    }
}

#[test]
fn a_unit_compiled_by_two_invocations_in_one_lane_is_waste_in_the_lane_output() {
    let unit = artifact("target/debug/deps/libserde-abc.rmeta");
    for (units, expected) in [
        (vec![unit.clone(), unit.clone()], true),
        (
            vec![
                unit.clone(),
                artifact("target/debug/deps/libserde-def.rmeta"),
            ],
            false,
        ),
        (vec![unit.clone()], false),
    ] {
        let mut value = quiet();
        let template = value["invocations"][0].clone();
        value["invocations"] = Value::Array(
            units
                .iter()
                .map(|artifact| {
                    let mut invocation = template.clone();
                    invocation["artifacts"] = json!([artifact]);
                    invocation
                })
                .collect(),
        );
        let found = waste(&metrics(value));
        assert_eq!(
            found
                .iter()
                .any(|line| line.contains("identical observed compilation unit executed 2 times")),
            expected,
            "{found:?}"
        );
    }
}

#[test]
fn mbx_summary_lines_are_parsed_with_and_without_the_optional_clauses() {
    let log = b"2026-11-12T08:51:39Z mbx[cache]: object cache: 0 hits, 0 misses, 567 not looked up, 4 bypassed; 0 B downloaded, 0 B uploaded, 1 MiB stored locally\n\
2026-11-12T08:51:40Z mbx[cache]: object cache: 43 hits, 2 misses; 0 B downloaded, 0 B uploaded, 0 B stored locally\n\
noise object cache: x hits\n";
    let found: Vec<_> = mbx_sessions(log)
        .into_iter()
        .map(|session| (session.hits, session.misses, session.not_looked_up))
        .collect();
    assert_eq!(found, [(0, 0, 567), (43, 2, 0)]);
}

#[test]
fn mbx_1_30_0_summary_without_the_object_cache_label_is_parsed() {
    // Printed by `mbx 1.30.0` on a local `cargo build -p velnor-cache-rules` (2026-11-20).
    let log = b"   Compiling velnor-cache-rules v0.0.0 (/checkout/crates/velnor-cache-rules)\n\
mbx[cache]: 1 hits, 0 misses; 0 B downloaded, 0 B uploaded, 0 B stored locally\n\
mbx[savings]: rustc believes it compiled everything. it is down 42d 20h across 102194 builds. let it believe.\n";
    let found: Vec<_> = mbx_sessions(log)
        .into_iter()
        .map(|session| (session.hits, session.misses, session.not_looked_up))
        .collect();
    assert_eq!(found, [(1, 0, 0)]);
}

#[test]
fn mbx_1_30_detail_lines_around_the_summary_are_not_unparsed() {
    let log = b"mbx[cache]: object cache: 1 hits, 89 misses, 12 not looked up; 0 B downloaded, 0 B uploaded, 77.5 MiB stored locally\n\
mbx[cache]: 27.42s session; 1.08s estimated compiler time avoided (summed across compilations)\n\
mbx[cache]: 12 compilations had no usable prior inputs or matching prediction for a lookup\n\
mbx[cache]: Cargo artifact reuse and CI cache archive transfers are not included above\n\
mbx[cache]: 662.5ms session; 0s estimated compiler time avoided (summed across compilations)\n";
    assert_eq!(mbx_sessions(log).len(), 1);
    assert!(
        mbx_unparsed_lines(log).is_empty(),
        "{:?}",
        mbx_unparsed_lines(log)
    );
}

#[test]
fn mbx_lookup_lines_that_do_not_parse_are_reported_and_never_counted_as_zero() {
    let log =
        b"2026-11-20T01:37:32Z mbx[cache]: object cache: 3 hits, many misses; 0 B downloaded\n\
mbx[cache]: 1 hits; 0 B downloaded, 0 B uploaded, 0 B stored locally\n\
mbx[cache]: 28.53s session; 2m 37s estimated compiler time avoided\n";
    assert_eq!(mbx_sessions(log).len(), 0);
    assert_eq!(
        mbx_unparsed_lines(log).len(),
        2,
        "{:?}",
        mbx_unparsed_lines(log)
    );
}

#[test]
fn an_empty_mbx_session_after_a_truncated_seed_is_not_waste() {
    for (truncated, expected) in [(false, true), (true, false)] {
        let mut value = quiet();
        value["invocations"][0]["artifacts"] =
            json!([artifact("target/debug/deps/libserde-abc.rlib")]);
        value["mbx"] = mbx(true, &[(0, 0, 567)]);
        value["mbx"]["seed_truncated"] = json!(truncated);
        let found = waste(&metrics(value));
        assert_eq!(
            found.iter().any(|line| line.contains("MBX object cache")),
            expected,
            "{truncated}: {found:?}"
        );
    }
}

#[test]
fn a_seed_stopped_at_the_startup_budget_is_read_from_the_job_log() {
    let stopped = b"2026-11-20T10:18:10Z ##[warning]MBX seeding stopped at the startup budget; the rest builds cold\n";
    let seeded = b"2026-11-20T10:18:10Z velnor: MBX seeded /t/mbx with 5 files, 9 bytes in 0.01s (0 already present)\n";
    assert!(mbx_seed_truncated(stopped));
    assert!(!mbx_seed_truncated(seeded));
}
