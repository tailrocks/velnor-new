// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::migration::{APP_SELECTION, app_and_lib_project, build, package_output, wrapped_build};
use super::nested_roots::nested_roots;
use super::{audit, command};

const FAILED_BUILD: &str = "An earlier inputs build failed: `cargo build` (failed commands record no receipts); fix that step first";

#[test]
fn a_failed_build_with_global_options_is_recorded_under_its_subcommand() {
    let repo = app_and_lib_project();
    let plain = build(&repo, false);
    assert!(plain.status.success(), "{plain:?}");
    for globals in [
        &["-v"][..],
        &["--color", "never"],
        &["--config", "build.jobs=1"],
    ] {
        let prepared = build(&repo, true);
        assert!(prepared.status.success(), "{globals:?}: {prepared:?}");
        let mut failing = vec!["cargo"];
        failing.extend_from_slice(globals);
        failing.extend_from_slice(&[
            "build",
            "-p",
            "lib",
            "--all-targets",
            "--offline",
            "--no-such-flag",
        ]);
        let failed = wrapped_build(&repo, &failing);
        assert!(!failed.status.success(), "{globals:?}: {failed:?}");
        let lib = package_output(&repo, "lib");
        fs::remove_file(lib.with_extension("velnor-inputs.json"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let refused = wrapped_build(&repo, &APP_SELECTION);
        assert_eq!(refused.status.code(), Some(1), "{globals:?}: {refused:?}");
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert!(stderr.contains(FAILED_BUILD), "{globals:?}: {refused:?}");
        assert!(!stderr.contains("build.jobs"), "{globals:?}: {refused:?}");
    }
}

#[test]
fn a_valid_build_with_global_options_is_recorded_and_audited() {
    for (globals, manifest) in [
        (&["-v"][..], &[][..]),
        (&["--color", "never"], &[]),
        (&["--config", "build.jobs=1"], &[]),
        (
            &["-Z", "unstable-options", "-C", "lib"],
            &["--manifest-path", "../Cargo.toml"][..],
        ),
    ] {
        let repo = app_and_lib_project();
        let mut cargo = vec!["cargo"];
        cargo.extend_from_slice(globals);
        cargo.extend_from_slice(&["build", "--workspace", "--all-targets", "--offline"]);
        cargo.extend_from_slice(manifest);
        let built = command(env!("CARGO_BIN_EXE_velnor-ci"))
            .env("RUSTC_BOOTSTRAP", "1")
            .current_dir(repo.root().join("rust"))
            .args(["inputs", "build", "--"])
            .args(&cargo)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(built.status.success(), "{globals:?}: {built:?}");
        let written = audit(&repo, &["--write"]);
        assert!(written.status.success(), "{globals:?}: {written:?}");
        assert_eq!(
            fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
                .unwrap_or_else(|e| unreachable!("{e}")),
            "shared\tapp\nshared\tlib\n"
        );
    }
}

#[test]
fn a_chdir_option_from_above_the_workspace_is_audited_in_the_workspace_it_names() {
    for chdir in [&["-C", "rust"][..], &["-C=rust"], &["-Crust"]] {
        let repo = app_and_lib_project();
        let mut cargo = vec!["cargo", "-Z", "unstable-options"];
        cargo.extend_from_slice(chdir);
        cargo.extend_from_slice(&["build", "--workspace", "--all-targets", "--offline"]);
        let built = command(env!("CARGO_BIN_EXE_velnor-ci"))
            .env("RUSTC_BOOTSTRAP", "1")
            .current_dir(repo.root())
            .args(["inputs", "build", "--"])
            .args(&cargo)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(built.status.success(), "{chdir:?}: {built:?}");
        let written = audit(&repo, &["--write"]);
        assert!(written.status.success(), "{chdir:?}: {written:?}");
        assert_eq!(
            fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
                .unwrap_or_else(|e| unreachable!("{e}")),
            "shared\tapp\nshared\tlib\n",
            "{chdir:?}"
        );
    }
}

#[test]
fn a_chdir_option_picks_the_nested_root_whose_step_the_build_belongs_to() {
    for chdir in [&["-C", "lab"][..], &["-C=lab"], &["-Clab"]] {
        let repo = nested_roots();
        let built = command(env!("CARGO_BIN_EXE_velnor-ci"))
            .env("RUSTC_BOOTSTRAP", "1")
            .current_dir(repo.root())
            .env("CARGO_TARGET_DIR", repo.root().join("lab/target"))
            .args(["inputs", "build", "--"])
            .arg(repo.root().join("driver/cargo"))
            .args(["-Z", "unstable-options"])
            .args(chdir)
            .args(["build", "--workspace"])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(built.status.success(), "{chdir:?}: {built:?}");
    }
}
