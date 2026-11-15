// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::timings::{TimingsReport, current_setup};
use velnor_model::{
    PackageName, RepoPath, RepoView, RustMetadata, RustPackage, Target, TargetKind, TargetName,
};

fn path(text: &str) -> RepoPath {
    RepoPath::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

fn view(timings: &str) -> RepoView {
    let mut view = RepoView::default();
    let manifest = path("Cargo.toml");
    let member = RustPackage {
        name: PackageName::new("a").unwrap_or_else(|e| unreachable!("{e}")),
        manifest: manifest.clone(),
        dependencies: Vec::new(),
        targets: vec![Target {
            name: TargetName::new("it").unwrap_or_else(|e| unreachable!("{e}")),
            kinds: vec![TargetKind::Test],
            tests: true,
            doctest: false,
            source: None,
            requires_features: false,
        }],
    };
    view.rust_metadata.insert(
        manifest,
        RustMetadata {
            members: vec![member],
        },
    );
    for (name, text) in [
        ("Cargo.toml", "[package]\nname = \"a\"\n"),
        ("Cargo.lock", ""),
        (
            "mise.toml",
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n",
        ),
        (".velnor/rust-ci.toml", "runner = \"arm-box\"\n"),
        (".velnor/rust-ci-timings.json", timings),
    ] {
        view.tracked_paths.insert(path(name));
        view.files.insert(path(name), text.as_bytes().to_vec());
    }
    view
}

#[test]
fn regenerating_does_not_consume_the_timings_file_it_replaces() {
    for timings in ["{", r#"{"gone/Cargo.toml": {}}"#] {
        let (roots, runner) =
            current_setup(&view(timings)).unwrap_or_else(|e| unreachable!("{timings}: {e}"));
        assert_eq!(roots, [path("Cargo.toml")], "{timings}");
        assert_eq!(runner.as_str(), "arm-box", "{timings}");
    }
}

#[test]
fn an_empty_report_removes_the_old_timings_file() {
    let dir = std::env::temp_dir().join(format!("velnor-ci-timings-stale-{}", std::process::id()));
    let file = dir.join(".velnor/rust-ci-timings.json");
    std::fs::create_dir_all(file.parent().unwrap_or(&dir)).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(&file, "{}").unwrap_or_else(|e| unreachable!("{e}"));
    let empty = TimingsReport {
        timings: BTreeMap::new(),
        notes: Vec::new(),
    };
    let message = velnor_ci::io::write_report(&dir, empty).unwrap_or_default();
    let left = file.exists();
    std::fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(message, ".velnor/rust-ci-timings.json was removed\n");
    assert!(!left);
}
