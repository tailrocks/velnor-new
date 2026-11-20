// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::{audit, command, own_units, workspace_below_the_root};
use velnor_test_support::real_cargo_str;

#[test]
fn compiler_flags_after_the_separator_remain_test_payload() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    repo.write_unstaged("driver/cargo.rs", r#"use std::io::Write;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "metadata") {
        std::process::exit(std::process::Command::new("@CARGO@").args(&args).status().unwrap().code().unwrap_or(1));
    }
    let compiler = args.iter().take_while(|arg| arg.as_str() != "--").any(|arg| arg == "--no-run");
    if !compiler {
        std::fs::OpenOptions::new().create(true).append(true).open("target/custom-tests-ran.txt").unwrap().write_all(b"run").unwrap();
    }
    std::fs::create_dir_all("target/generated").unwrap();
    let value = std::path::Path::new("target/generated/value.txt");
    if !value.exists() { std::fs::write(value, "input").unwrap(); }
    let counter = std::path::Path::new("target/compiler-passes.txt");
    let pass = std::fs::read_to_string(counter).unwrap_or_default().parse::<u32>().unwrap_or(0) + 1;
    std::fs::write(counter, pass.to_string()).unwrap();
    std::fs::write("target/debug/build/lib-f000000000000008/output", format!("cargo:rerun-if-changed=../target/generated\ncargo:rustc-env=FIXTURE_COMPILER_PASS={pass}\n")).unwrap();
}
"#
    .replace("@CARGO@", &real_cargo_str()).as_str());
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
        fs::read_to_string(repo.root().join("rust/target/compiler-passes.txt"))
            .unwrap_or_else(|error| unreachable!("{error}")),
        "3"
    );
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

#[cfg(unix)]
#[test]
fn audit_wrapped_nextest_cannot_forge_step_outputs() {
    use std::os::unix::fs::PermissionsExt;

    let repo = workspace_below_the_root();
    let driver = repo.root().join("driver");
    fs::create_dir_all(&driver).unwrap_or_else(|e| unreachable!("{e}"));
    let nextest = driver.join("cargo-nextest");
    fs::write(&nextest, "#!/bin/sh\nenv > \"$RUNNER_TEMP/child-env\"\nif [ -n \"$GITHUB_OUTPUT\" ]; then echo tests_run=999 >> \"$GITHUB_OUTPUT\"; fi\necho '     Summary [   0.001s] 7 tests run: 7 passed, 2 skipped' >&2\n").unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&nextest, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let output_file = driver.join("output");
    fs::write(&output_file, "existing=1\n").unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        driver.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut run = command(env!("CARGO_BIN_EXE_velnor-ci"));
    run.current_dir(repo.root().join("rust"))
        .args([
            "run",
            "--",
            env!("CARGO_BIN_EXE_velnor-ci"),
            "inputs",
            "build",
            "--",
            "cargo-nextest",
            "nextest",
            "run",
            "--no-tests=pass",
        ])
        .env("PATH", path)
        .env("RUNNER_TEMP", &driver);
    for name in [
        "GITHUB_OUTPUT",
        "GITHUB_ENV",
        "GITHUB_PATH",
        "GITHUB_STEP_SUMMARY",
        "GITHUB_STATE",
    ] {
        run.env(name, &output_file);
    }
    let output = run.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let written = fs::read_to_string(&output_file).unwrap_or_else(|e| unreachable!("{e}"));
    // The wrapper writes `built=` itself after a nextest run exits 0. The child's
    // forged `tests_run=999` line must still be absent: only the wrapper's lines remain.
    assert_eq!(
        crate::run_nextest::without_built(&written),
        "existing=1\ntests_run=7\ntests_listed=9\n"
    );
    assert!(
        written
            .lines()
            .all(|line| !line.starts_with("built=") || line == "built=true"),
        "{written:?}"
    );
    let environment =
        fs::read_to_string(driver.join("child-env")).unwrap_or_else(|e| unreachable!("{e}"));
    for name in [
        "GITHUB_OUTPUT",
        "GITHUB_ENV",
        "GITHUB_PATH",
        "GITHUB_STEP_SUMMARY",
        "GITHUB_STATE",
    ] {
        assert!(
            !environment
                .lines()
                .any(|line| line.starts_with(&format!("{name}="))),
            "{name} leaked"
        );
    }
}
