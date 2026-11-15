// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MISE: &str =
    "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("velnor-ci-prepare-{}-{name}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    }
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    dir
}

fn reported(rust: &str, nextest: Option<&str>) -> String {
    let entry = |version: &str| {
        format!(
            "[{{\"version\":\"{version}\",\"requested_version\":\"{version}\",\"installed\":true,\"active\":true}}]"
        )
    };
    let mut tools = vec![format!("\"rust\":{}", entry(rust))];
    if let Some(version) = nextest {
        tools.push(format!(
            "\"aqua:nextest-rs/nextest/cargo-nextest\":{}",
            entry(version)
        ));
    }
    format!("{{{}}}", tools.join(","))
}

struct Run {
    output: Output,
    env_file: String,
    outputs: String,
}

fn prepare(name: &str, json: &str) -> Run {
    let dir = scratch(name);
    let checkout = dir.join("checkout");
    fs::create_dir_all(&checkout).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(checkout.join("mise.toml"), MISE).unwrap_or_else(|e| unreachable!("{e}"));
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap_or_else(|e| unreachable!("{e}"));
    let mise = bin.join("mise");
    fs::write(&mise, format!("#!/bin/sh\ncat <<'EOF'\n{json}\nEOF\n"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&mise, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (env_file, outputs) = (dir.join("env"), dir.join("output"));
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("prepare")
        .current_dir(&checkout)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("GITHUB_ENV", &env_file)
        .env("GITHUB_OUTPUT", &outputs)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let read = |file: &Path| fs::read_to_string(file).unwrap_or_default();
    Run {
        output,
        env_file: read(&env_file),
        outputs: read(&outputs),
    }
}

#[test]
fn prepare_sets_the_build_profile_and_an_empty_compat_when_every_tool_is_at_its_pin() {
    let run = prepare("ok", &reported("1.99.1", Some("0.9.151")));
    assert!(run.output.status.success(), "{:?}", run.output);
    assert_eq!(
        run.env_file,
        "RUSTUP_AUTO_INSTALL=0\nCARGO_INCREMENTAL=0\nCARGO_TERM_COLOR=always\nSOURCE_DATE_EPOCH=0\n"
    );
    assert_eq!(run.outputs, "compat=\n");
}

#[test]
fn prepare_names_the_tool_that_is_not_at_its_pinned_version_and_writes_nothing() {
    for (name, json, tool) in [
        (
            "wrong",
            reported("1.98.0", Some("0.9.151")),
            "rust: pinned 1.99.1, mise reports 1.98.0",
        ),
        (
            "absent",
            reported("1.99.1", None),
            "aqua:nextest-rs/nextest/cargo-nextest: pinned 0.9.151, mise reports nothing installed",
        ),
    ] {
        let run = prepare(name, &json);
        assert_eq!(run.output.status.code(), Some(1), "{name}");
        assert!(
            String::from_utf8_lossy(&run.output.stderr).contains(tool),
            "{name}: {:?}",
            run.output
        );
        assert_eq!(
            (run.env_file.as_str(), run.outputs.as_str()),
            ("", ""),
            "{name}"
        );
    }
}

#[test]
fn restored_exits_zero_and_writes_nothing() {
    let dir = scratch("restored");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("restored")
        .current_dir(&dir)
        .env("GITHUB_ENV", dir.join("env"))
        .env("GITHUB_OUTPUT", dir.join("output"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    assert!(!dir.join("env").exists() && !dir.join("output").exists());
}
