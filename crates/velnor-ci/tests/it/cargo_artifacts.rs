// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_ci::cargo_artifacts::{ArtifactTap, CargoArtifact, CargoProgress, ProgressKind};

pub(crate) fn record(fresh: bool) -> serde_json::Value {
    json!({
        "reason": "compiler-artifact",
        "package_id": "path+file:///工作/app#0.1.0",
        "manifest_path": "/工作/app/Cargo.toml",
        "target": {
            "name": "app", "kind": ["lib"], "crate_types": ["rlib"],
            "src_path": "/工作/app/src/lib.rs", "edition": "2024"
        },
        "profile": {
            "opt_level": "0", "debuginfo": "line-tables-only",
            "debug_assertions": true, "overflow_checks": true, "test": false
        },
        "features": ["default"], "filenames": ["/工作/target/debug/deps/libapp-opaque.rlib"],
        "fresh": fresh, "executable": null, "future_field": {"anything": true}
    })
}

#[test]
fn artifacts_preserve_cargo_identity_across_every_chunk_size() {
    let expected: Vec<CargoArtifact> = [false, true]
        .into_iter()
        .map(|fresh| serde_json::from_value(record(fresh)).expect("valid Cargo artifact"))
        .collect();
    let input = format!(
        "Compiling app\n  Compiling app v0.1.0 (/工作/app dir)\n{}\nPASS [ 0.125s] (1/2) app::suite test_name\nTRY 1 FAIL [0.250s] app::suite flaky\nTRY 2 PASS [0.500s] app::suite flaky\nLEAK [0.103s] app::suite leak\nTRY 1 LEAK [0.104s] app::suite retry_leak\nLEAK-FAIL [0.105s] app::suite leak_fail\nTRY 2 LEAK-FAIL [0.106s] app::suite retry_leak_fail\nFLAKY 2/2 [0.750s] app::suite flaky\n{{\"reason\":\"build-finished\",\"success\":true}}\n{}",
        record(false),
        record(true)
    );
    for size in 1..=input.len() {
        let mut tap = ArtifactTap::default();
        for chunk in input.as_bytes().chunks(size) {
            tap.feed(chunk);
        }
        let batch = tap.finish();
        assert_eq!(batch.artifacts, expected, "chunk size {size}");
        assert_eq!(batch.rejected, 0, "chunk size {size}");
        assert_eq!(
            batch.progress,
            [CargoProgress {
                kind: ProgressKind::Compiling,
                package: "app".to_owned(),
                version: "0.1.0".to_owned(),
                location: Some("/工作/app dir".to_owned()),
            }],
            "chunk size {size}"
        );
        assert_eq!(
            batch.tests,
            [
                ("app::suite test_name".to_owned(), 0.125),
                ("app::suite flaky".to_owned(), 0.250),
                ("app::suite flaky".to_owned(), 0.500),
                ("app::suite leak".to_owned(), 0.103),
                ("app::suite retry_leak".to_owned(), 0.104),
                ("app::suite leak_fail".to_owned(), 0.105),
                ("app::suite retry_leak_fail".to_owned(), 0.106)
            ],
            "chunk size {size}"
        );
    }
    assert_eq!(expected[0].package_id, "path+file:///工作/app#0.1.0");
    assert_eq!(expected[0].target.src_path, "/工作/app/src/lib.rs");
    assert_eq!(expected[0].profile.debuginfo, "line-tables-only");
    assert!(!expected[0].fresh);
    assert!(expected[1].fresh);
    let mut draining = ArtifactTap::default();
    draining.feed(format!("{}\n{}", record(false), record(true)).as_bytes());
    assert_eq!(draining.take_ready().artifacts, expected[..1]);
    assert_eq!(draining.take_ready().artifacts, []);
    assert_eq!(draining.finish().artifacts, expected[1..]);
}

#[test]
fn invalid_and_oversized_lines_do_not_hide_the_next_artifact() {
    let mut missing = record(false);
    missing
        .as_object_mut()
        .expect("object")
        .remove("package_id");
    let mut wrong_type = record(false);
    wrong_type["fresh"] = json!("true");
    let cases = [
        (
            "plain text\n{broken\nnull\n{\"reason\":\"compiler-message\"}\nCompiling app vbad\nPASS [NaNs] binary test\nLEAK [NaNs] binary test\nLEAK [infs] binary test\nLEAK-FAIL [-1s] binary test\nTRY 0 LEAK [1s] binary test\nTRY nope LEAK-FAIL [1s] binary test\nTRY 0 PASS [1s] binary test\nTRY -1 FAIL [1s] binary test\nTRY nope PASS [1s] binary test\nTRY 1 FLAKY [1s] binary test\n".to_owned(),
            0,
        ),
        (format!("{missing}\n{wrong_type}\n"), 2),
        (format!("{}\n", "x".repeat(1024 * 1024 + 1)), 1),
        (format!("{}\n", " ".repeat(1024 * 1024 + 1)), 1),
    ];
    for (prefix, rejected) in cases {
        let mut tap = ArtifactTap::default();
        for chunk in prefix.as_bytes().chunks(127) {
            tap.feed(chunk);
        }
        tap.feed(record(true).to_string().as_bytes());
        tap.feed(b"\n{\"reason\":\"compiler-artifact\"");
        let batch = tap.finish();
        assert_eq!(batch.rejected, rejected);
        assert_eq!(batch.artifacts.len(), 1);
        assert!(batch.artifacts[0].fresh);
        assert_eq!(batch.progress, Vec::<CargoProgress>::new());
        assert_eq!(batch.tests, Vec::<(String, f64)>::new());
    }
    let mut unfinished = ArtifactTap::default();
    unfinished.feed(&vec![b' '; 1024 * 1024 + 1]);
    unfinished.feed(record(true).to_string().as_bytes());
    assert_eq!(unfinished.finish().artifacts, Vec::<CargoArtifact>::new());
    for (prefix, kind) in [
        ("Checking", ProgressKind::Checking),
        ("Documenting", ProgressKind::Documenting),
        ("Downloaded", ProgressKind::Downloaded),
    ] {
        let mut tap = ArtifactTap::default();
        tap.feed(format!("{prefix} dep v1.2.3\nFAIL [0.5s] bin fail\n").as_bytes());
        let batch = tap.finish();
        assert_eq!(batch.progress[0].kind, kind);
        assert_eq!(batch.progress[0].version, "1.2.3");
        assert_eq!(batch.progress[0].location, None);
        assert_eq!(batch.tests, [("bin fail".to_owned(), 0.5)]);
    }
}

#[test]
fn oversized_cargo_artifacts_report_rejection_once_across_chunks_and_eof() {
    let mut oversized = record(false);
    oversized["filenames"] = json!(["x".repeat(1024 * 1024)]);
    for newline in [false, true] {
        let input = format!("{oversized}{}", if newline { "\n" } else { "" });
        for size in [127, input.len()] {
            let mut tap = ArtifactTap::default();
            let mut rejected = 0;
            for chunk in input.as_bytes().chunks(size) {
                tap.feed(chunk);
                rejected += tap.take_ready().rejected;
            }
            if newline {
                tap.feed(record(true).to_string().as_bytes());
            }
            let batch = tap.finish();
            assert_eq!(rejected + batch.rejected, 1);
            assert_eq!(batch.artifacts.len(), usize::from(newline));
            assert!(batch.artifacts.iter().all(|artifact| artifact.fresh));
        }
    }
}

#[test]
fn text_report_names_each_failed_package_in_every_target_form() {
    let input = [
        "error: could not compile `alpha` (lib) due to 1 previous error",
        "error: could not compile `beta` (bin \"beta-cli\") due to 2 previous errors; 1 warning emitted",
        "error: could not compile `gamma` (lib test) due to previous error",
        "error: could not compile `delta` (test \"suite\") due to previous error",
        "error: could not compile `epsilon`",
        "warning: build failed, waiting for other jobs to finish...",
        "   Compiling zeta v1.0.0",
    ]
    .join("\n");
    let mut tap = ArtifactTap::default();
    tap.feed(input.as_bytes());
    let names: Vec<_> = tap
        .finish()
        .failed_units
        .into_iter()
        .map(|unit| (unit.package, unit.version))
        .collect();
    assert_eq!(
        names,
        [
            ("alpha".to_owned(), None),
            ("beta".to_owned(), None),
            ("gamma".to_owned(), None),
            ("delta".to_owned(), None),
            ("epsilon".to_owned(), None),
        ]
    );
}
