// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The audit of a target directory in the layout Cargo 1.100 writes. The unit files are the
//! ones of `own_units`, moved to `build/<package>/<hash>/{fingerprint,out,run}` the way a real
//! 1.100 build places them.

use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use super::{OWN_UNITS, audit, placed, unit, workspace_below_the_root};
use crate::io::Repo;

/// The `(package, hash)` of an old `<package>-<hash>` folder.
fn split(folder: &str) -> (&str, &str) {
    folder
        .rsplit_once('-')
        .unwrap_or_else(|| unreachable!("{folder}"))
}

fn own_units_1100(repo: &Repo, target: &str, left_out: &[&str]) {
    placed(repo, target);
    for (key, _, file, text, fingerprint) in OWN_UNITS {
        if left_out.contains(&key) {
            continue;
        }
        // `deps/<name>-<hash>.d` belongs to the unit of its fingerprint folder,
        // `build/<package>-<hash>/<file>` to its own.
        let (folder, name) = match (file.split_once('/'), fingerprint.split_once('/')) {
            (Some(("deps", name)), Some((folder, _))) => (folder, name),
            (Some(("build", rest)), _) => rest
                .split_once('/')
                .unwrap_or_else(|| unreachable!("{file}")),
            _ => unreachable!("{file}"),
        };
        let (package, hash) = split(folder);
        let base = format!("{target}/build/{package}/{hash}");
        let dep_info = format!("out.d: {text}\n");
        if name == "output" {
            unit(repo, &format!("{base}/run/stdout"), "");
        } else if name.starts_with("build_script_build") {
            unit(repo, &format!("{base}/out/build_script_build.d"), &dep_info);
        } else {
            unit(repo, &format!("{base}/out/{name}"), &dep_info);
        }
        if let Some((_, kind)) = fingerprint.split_once('/') {
            unit(repo, &format!("{base}/fingerprint/{kind}"), "");
        }
    }
}

#[test]
fn a_target_directory_in_the_layout_of_cargo_1100_is_audited_like_the_old_one() {
    let repo = workspace_below_the_root();
    own_units_1100(&repo, "rust/target/debug", &[]);
    let output = "rust/target/debug/build/lib/f000000000000008/run/stdout";
    unit(&repo, output, "cargo:rerun-if-changed=../..\n");
    let built = UNIX_EPOCH + Duration::from_secs(3_000_000_001);
    repo.set_mtime(output, built);
    super::inventory::build(&repo, output, None);

    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        ".\tlib\n"
    );
}

#[test]
fn a_unit_without_dep_info_in_the_layout_of_cargo_1100_makes_the_audit_refuse() {
    let repo = workspace_below_the_root();
    own_units_1100(&repo, "rust/target/debug", &["lib"]);
    let refused = audit(&repo, &["--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("found no current dep-info in") && stderr.contains("lib"),
        "{stderr}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_built_target_directory_in_a_layout_with_no_known_unit_makes_the_audit_refuse() {
    let repo = workspace_below_the_root();
    unit(&repo, "rust/target/debug/units/lib/x/liblib.rlib", "");
    unit(&repo, "rust/target/debug/units/lib/x/liblib.rmeta", "");
    let refused = audit(&repo, &["--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("found no units in") && stderr.contains("unknown cargo layout?"),
        "{stderr}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}
