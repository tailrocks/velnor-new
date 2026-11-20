// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

// Cargo skips the unit tests of a target that sets `test = false`, so a
// `#[cfg(test)]` module in such a target never compiles or runs (#1366).
// This guard fails when a workspace crate disables a target whose source
// still holds unit tests.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Target {
    header: String,
    path: Option<String>,
    test_disabled: bool,
}

fn targets(manifest: &str) -> Vec<Target> {
    let mut found: Vec<Target> = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            let header = line.trim_matches(['[', ']']).to_owned();
            if line == "[lib]" || line == "[[bin]]" {
                found.push(Target {
                    header,
                    ..Target::default()
                });
            } else {
                found.push(Target::default());
            }
            continue;
        }
        let Some(target) = found.last_mut().filter(|t| !t.header.is_empty()) else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match (key.trim(), value.trim()) {
            ("test", "false") => target.test_disabled = true,
            ("path", value) => target.path = Some(value.trim_matches('"').to_owned()),
            _ => {}
        }
    }
    found.retain(|t| !t.header.is_empty());
    found
}

fn cfg_test_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            cfg_test_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = fs::read_to_string(&path).unwrap_or_default();
            if text.contains("#[cfg(test)]") {
                out.push(path);
            }
        }
    }
}

/// Every `(crate directory, disabled target)` whose source holds `#[cfg(test)]` code.
/// A disabled `[lib]` owns every file under `src/` except the `[[bin]]` entry files;
/// a disabled `[[bin]]` owns only its own entry file.
fn offenders(crates: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(crates) else {
        return out;
    };
    for entry in entries.flatten() {
        let krate = entry.path();
        let Ok(manifest) = fs::read_to_string(krate.join("Cargo.toml")) else {
            continue;
        };
        let bins: Vec<String> = targets(&manifest)
            .iter()
            .filter(|t| t.header == "bin")
            .filter_map(|t| t.path.clone())
            .collect();
        for target in targets(&manifest).iter().filter(|t| t.test_disabled) {
            let mut files = Vec::new();
            if target.header == "lib" {
                let mut all = Vec::new();
                cfg_test_files(&krate.join("src"), &mut all);
                files.extend(all.into_iter().filter(|f| {
                    !bins
                        .iter()
                        .any(|b| f.ends_with(Path::new(b).file_name().unwrap_or_default()))
                }));
            } else if let Some(path) = &target.path {
                let entry_file = krate.join(path);
                if fs::read_to_string(&entry_file)
                    .unwrap_or_default()
                    .contains("#[cfg(test)]")
                {
                    files.push(entry_file);
                }
            }
            for file in files {
                out.push(format!(
                    "{}: test = false on {} but {} has #[cfg(test)]",
                    krate.display(),
                    target.header,
                    file.display()
                ));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn no_workspace_crate_disables_a_target_that_holds_unit_tests() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../");
    let offenders = offenders(&crates);
    assert!(
        offenders.is_empty(),
        "these targets never run their unit tests; enable them or move the tests:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_guard_flags_a_disabled_lib_that_holds_cfg_test_code() {
    let root = std::env::temp_dir().join(format!("velnor-unit-targets-{}", std::process::id()));
    let krate = root.join("crates/dead");
    fs::create_dir_all(krate.join("src")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(
        krate.join("Cargo.toml"),
        "[package]\nname = \"dead\"\n\n[lib]\npath = \"src/lib.rs\"\ntest = false\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(
        krate.join("src/lib.rs"),
        "#[cfg(test)]\nmod tests { #[test] fn it_runs() {} }\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let found = offenders(&root.join("crates"));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].contains("src/lib.rs"), "{found:#?}");
}
