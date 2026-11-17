// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::{audit, command, own_units, workspace_below_the_root};

#[test]
fn compiler_flags_after_the_separator_remain_test_payload() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    repo.write_unstaged("driver/cargo.rs", r#"use std::io::Write;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "metadata") {
        std::process::exit(std::process::Command::new("cargo").args(&args).status().unwrap().code().unwrap_or(1));
    }
    let compiler = args.iter().take_while(|arg| arg.as_str() != "--").any(|arg| arg == "--no-run");
    if !compiler {
        std::fs::OpenOptions::new().create(true).append(true).open("target/custom-tests-ran.txt").unwrap().write_all(b"run").unwrap();
    }
    std::fs::create_dir_all("target/generated").unwrap();
    let value = std::path::Path::new("target/generated/value.txt");
    if !value.exists() { std::fs::write(value, "input").unwrap(); }
    std::fs::write("target/debug/build/lib-f000000000000008/output", "cargo:rerun-if-changed=../target/generated\n").unwrap();
}
"#);
    let driver = repo.root().join("driver/cargo");
    let compiled = command("rustc")
        .current_dir(repo.root())
        .args(["--edition=2024", "driver/cargo.rs", "-o"])
        .arg(&driver)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(compiled.status.success(), "{compiled:?}");
    let built = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join("rust"))
        .args(["inputs", "build", "--"])
        .arg(&driver)
        .args(["test", "--workspace", "--", "--no-run"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join("rust/target/custom-tests-ran.txt"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "run"
    );
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        ""
    );
}

#[test]
fn an_arbitrary_commands_manifest_argument_is_passed_through_unchanged() {
    let repo = workspace_below_the_root();
    let echoed = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .args([
            "inputs",
            "build",
            "--",
            "echo",
            "--manifest-path=/nonexistent/Cargo.toml",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(echoed.status.success(), "{echoed:?}");
    assert_eq!(echoed.stdout, b"--manifest-path=/nonexistent/Cargo.toml\n");
}
