// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Output;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{
    audit, own_units, own_units_in, path_dependency_units_in, placed,
    root_package_with_path_dependencies, unit, workspace_below_the_root,
};
use crate::io::Repo;

const DEBUG: &str = "rust/target/debug";

fn refuses(output: &Output, text: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(text),
        "{output:?}"
    );
    assert!(
        !output.status.success() && !String::from_utf8_lossy(&output.stdout).contains("wrote"),
        "{output:?}"
    );
}

fn written(repo: &Repo) -> bool {
    repo.root().join(".velnor/rust-inputs.lock").exists()
}

#[test]
fn a_build_script_run_with_a_fingerprint_and_no_output_makes_the_audit_refuse() {
    let repo = workspace_below_the_root();
    own_units(&repo, DEBUG, &[]);
    unit(
        &repo,
        &format!("{DEBUG}/.fingerprint/lib-f0000000000000e1/run-build-script-build-script-build"),
        "",
    );
    refuses(
        &audit(&repo, &["--write"]),
        &format!(
            "/{DEBUG} for lib target  (build script run with a cargo fingerprint and no output (unit f0000000000000e1));"
        ),
    );
    assert!(!written(&repo));

    unit(
        &repo,
        &format!("{DEBUG}/build/lib-f0000000000000e1/output"),
        "",
    );
    let complete = audit(&repo, &["--write"]);
    assert!(complete.status.success(), "{complete:?}");
    assert!(written(&repo));
}

#[test]
fn a_custom_target_directory_is_checked_when_cargo_marks_it_and_a_nested_target_directory_is_not() {
    let repo = workspace_below_the_root();
    let custom = "rust/target/custom/debug";
    own_units_in(&repo, DEBUG, custom, &[]);
    unit(&repo, "rust/target/tool/CACHEDIR.TAG", "");
    unit(&repo, "rust/target/tool/.rustc_info.json", "{}");
    unit(
        &repo,
        "rust/target/tool/debug/deps/lib-f00000000000000e.d",
        "out.d: lib/src/lib.rs\n",
    );
    unit(
        &repo,
        "rust/target/tool/debug/.fingerprint/lib-f00000000000000e/test-lib-lib",
        "",
    );
    let complete = audit(&repo, &["--write"]);
    assert!(complete.status.success(), "{complete:?}");

    fs::remove_file(
        repo.root()
            .join(format!("{custom}/deps/it-f000000000000006.d")),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    refuses(
        &audit(&repo, &["--write"]),
        &format!("/{custom} for lib (rust/lib/Cargo.toml)"),
    );
}

#[test]
fn a_stale_host_unit_of_a_member_used_as_a_build_dependency_makes_the_audit_refuse() {
    let repo = workspace_below_the_root();
    repo.write(
        "rust/app/Cargo.toml",
        "[package]\nname = \"app\"\nedition = \"2024\"\n\n[build-dependencies]\nlib = { path = \"../lib\" }\n",
    );
    let triple = "rust/target/x86_64-unknown-linux-gnu/debug";
    own_units_in(&repo, DEBUG, triple, &[]);
    let host = "rust/target/debug/deps/lib-f0000000000000b1.d";
    unit(&repo, host, "out.d: lib/src/lib.rs\n");
    unit(
        &repo,
        &format!("{DEBUG}/.fingerprint/lib-f0000000000000b1/lib-lib"),
        "",
    );
    unit(
        &repo,
        &format!("{DEBUG}/build/lib-f0000000000000b2/build_script_build-f0000000000000b2.d"),
        "out.d: lib/build.rs\n",
    );
    unit(
        &repo,
        &format!("{DEBUG}/build/lib-f0000000000000b3/output"),
        "",
    );
    let complete = audit(&repo, &["--write"]);
    assert!(complete.status.success(), "{complete:?}");

    repo.set_mtime(host, UNIX_EPOCH + Duration::from_secs(1_000_000_000));
    refuses(
        &audit(&repo, &["--write"]),
        &format!(
            "/{triple} for lib (rust/lib/Cargo.toml) target lib (rust/lib/src/lib.rs, 2 of 3 units);"
        ),
    );
}

#[test]
fn a_configuration_known_only_from_fingerprints_without_dep_info_makes_the_audit_refuse() {
    let repo = workspace_below_the_root();
    let triple = "rust/target/x86_64-unknown-linux-gnu/debug";
    own_units_in(&repo, DEBUG, triple, &[]);
    own_units(&repo, DEBUG, &[]);
    for output in ["app-f000000000000004", "lib-f000000000000008"] {
        repo.set_mtime(
            &format!("{triple}/build/{output}/output"),
            SystemTime::now() + Duration::from_secs(100),
        );
    }
    let complete = audit(&repo, &["--write"]);
    assert!(complete.status.success(), "{complete:?}");

    for entry in
        fs::read_dir(repo.root().join(triple).join("deps")).unwrap_or_else(|e| unreachable!("{e}"))
    {
        fs::remove_file(entry.unwrap_or_else(|e| unreachable!("{e}")).path())
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    refuses(&audit(&repo, &["--write"]), &format!("/{triple} for app"));
}

fn build_dependency_on_lib(repo: &Repo) {
    repo.write(
        "rust/app/Cargo.toml",
        "[package]\nname = \"app\"\nedition = \"2024\"\n\n[build-dependencies]\nlib = { path = \"../lib\" }\n",
    );
    repo.git(&["add", "--all"]);
}

#[test]
fn the_host_build_script_run_of_a_build_dependency_member_is_required_in_the_host_directory() {
    let repo = workspace_below_the_root();
    build_dependency_on_lib(&repo);
    let triple = "rust/target/x86_64-unknown-linux-gnu/debug";
    own_units_in(&repo, DEBUG, triple, &[]);
    unit(
        &repo,
        &format!("{DEBUG}/deps/lib-f0000000000000b1.d"),
        "out.d: lib/src/lib.rs\n",
    );
    unit(
        &repo,
        &format!("{DEBUG}/.fingerprint/lib-f0000000000000b1/lib-lib"),
        "",
    );
    let target_only = audit(&repo, &["--write"]);
    refuses(
        &target_only,
        &format!(
            "/{triple} for lib (rust/lib/Cargo.toml) target build-script-build (rust/lib/build.rs, 1 of 2 units);"
        ),
    );
    assert!(!written(&repo));

    unit(
        &repo,
        &format!("{DEBUG}/build/lib-f0000000000000b3/output"),
        "",
    );
    let host_run = audit(&repo, &["--write"]);
    assert!(host_run.status.success(), "{host_run:?}");
}

#[test]
fn an_orphan_unit_of_a_member_in_a_target_directory_makes_that_directory_a_candidate() {
    let repo = workspace_below_the_root();
    let triple = "rust/target/x86_64-unknown-linux-gnu/debug";
    placed(&repo, triple);
    for (configuration, package, hash) in [
        (DEBUG, "app", "f0000000000000c1"),
        (DEBUG, "lib", "f0000000000000c2"),
        (triple, "lib", "f0000000000000c3"),
    ] {
        unit(
            &repo,
            &format!("{configuration}/.fingerprint/{package}-{hash}/lib-{package}"),
            "",
        );
    }
    refuses(
        &audit(&repo, &["--write"]),
        &format!("/{triple} for app (rust/app/Cargo.toml)"),
    );
    assert!(!written(&repo));
}

#[test]
fn a_host_copy_of_a_build_dependency_member_does_not_stand_in_for_its_target_library() {
    let repo = workspace_below_the_root();
    build_dependency_on_lib(&repo);
    let triple = "rust/target/x86_64-unknown-linux-gnu/debug";
    own_units_in(&repo, DEBUG, triple, &["lib"]);
    unit(
        &repo,
        &format!("{DEBUG}/deps/lib-f0000000000000b1.d"),
        "out.d: lib/src/lib.rs\n",
    );
    unit(
        &repo,
        &format!("{DEBUG}/.fingerprint/lib-f0000000000000b1/lib-lib"),
        "",
    );
    unit(
        &repo,
        &format!("{DEBUG}/build/lib-f0000000000000b3/output"),
        "",
    );
    refuses(
        &audit(&repo, &["--write"]),
        &format!(
            "/{triple} for lib (rust/lib/Cargo.toml) target lib (rust/lib/src/lib.rs, 1 of 3 units);"
        ),
    );
    assert!(!written(&repo));
}

#[test]
fn a_stale_build_script_run_is_not_covered_by_another_current_run_of_the_same_script() {
    let repo = workspace_below_the_root();
    own_units(&repo, DEBUG, &[]);
    for (hash, rerun) in [
        ("f0000000000000e1", ""),
        ("f0000000000000e2", "cargo:rerun-if-changed=data.txt\n"),
    ] {
        unit(
            &repo,
            &format!("{DEBUG}/.fingerprint/lib-{hash}/run-build-script-build-script-build"),
            "",
        );
        unit(&repo, &format!("{DEBUG}/build/lib-{hash}/output"), rerun);
    }
    unit(&repo, "rust/lib/data.txt", "");
    repo.set_mtime(
        "rust/lib/data.txt",
        SystemTime::now() + Duration::from_secs(100),
    );
    refuses(
        &audit(&repo, &["--write"]),
        &format!(
            "/{DEBUG} for lib target  (output stale: input data.txt changed after it ran (unit f0000000000000e2);"
        ),
    );
    assert!(!written(&repo));
}

#[test]
fn a_member_that_an_excluded_path_package_uses_as_a_build_dependency_needs_its_host_unit() {
    let repo = root_package_with_path_dependencies();
    repo.write(
        "vendor/mid/Cargo.toml",
        "[package]\nname = \"mid\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[dependencies]\nleaf = { path = \"../leaf\" }\n\n[build-dependencies]\nother = { path = \"../../other\" }\n",
    );
    repo.git(&["add", "--all"]);
    let triple = "target/x86_64-unknown-linux-gnu/debug";
    path_dependency_units_in(&repo, triple);
    unit(
        &repo,
        "target/debug/deps/leaf-000000000000001f.d",
        "out.d: vendor/leaf/src/lib.rs\n",
    );
    unit(
        &repo,
        "target/debug/.fingerprint/leaf-000000000000001f/lib-leaf",
        "",
    );
    refuses(
        &audit(&repo, &["--write"]),
        &format!(
            "/{triple} for other (other/Cargo.toml) target other (other/src/lib.rs, 2 of 3 units);"
        ),
    );

    unit(
        &repo,
        "target/debug/deps/other-000000000000001e.d",
        "out.d: other/src/lib.rs\n",
    );
    unit(
        &repo,
        "target/debug/.fingerprint/other-000000000000001e/lib-other",
        "",
    );
    let complete = audit(&repo, &["--write"]);
    assert!(complete.status.success(), "{complete:?}");
}
