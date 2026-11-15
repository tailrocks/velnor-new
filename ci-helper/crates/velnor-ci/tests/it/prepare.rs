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

const RUSTC: &str =
    "rustc 1.99.1 (abc 2026-08-27)\nbinary: rustc\nhost: aarch64-apple-darwin\nrelease: 1.99.1\n";
const DEPENDENCY_DEBUG: &str = "[profile.dev.package.\"*\"]\ndebug = \"none\"\n\n[profile.test.package.\"*\"]\ndebug = \"none\"\n";

struct Setup {
    json: String,
    rustc: bool,
    rust_ci_toml: Option<&'static str>,
    cargo_config: Option<&'static str>,
    env: Vec<(&'static str, &'static str)>,
}

impl Setup {
    fn new(json: String) -> Self {
        Self {
            json,
            rustc: true,
            rust_ci_toml: None,
            cargo_config: None,
            env: Vec::new(),
        }
    }
}

struct Run {
    output: Output,
    env_file: String,
    outputs: String,
    cargo_config: String,
    velnor_config: String,
}

fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn prepare_in(dir: &Path, setup: &Setup) -> Run {
    let checkout = dir.join("checkout");
    let bin = dir.join("bin");
    let cargo_home = dir.join("cargo-home");
    fs::create_dir_all(checkout.join(".velnor")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(&bin).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(checkout.join("mise.toml"), MISE).unwrap_or_else(|e| unreachable!("{e}"));
    let ci_toml = checkout.join(".velnor/rust-ci.toml");
    match setup.rust_ci_toml {
        Some(text) => fs::write(ci_toml, text).unwrap_or_else(|e| unreachable!("{e}")),
        None => drop(fs::remove_file(ci_toml)),
    }
    if let Some(text) = setup.cargo_config {
        fs::create_dir_all(&cargo_home).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(cargo_home.join("config.toml"), text).unwrap_or_else(|e| unreachable!("{e}"));
    }
    script(
        &bin.join("mise"),
        &format!("cat <<'EOF'\n{}\nEOF", setup.json),
    );
    if setup.rustc {
        script(&bin.join("rustc"), &format!("cat <<'EOF'\n{RUSTC}EOF"));
    }
    let (env_file, outputs) = (dir.join("env"), dir.join("output"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .arg("prepare")
        .current_dir(&checkout)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("CARGO_HOME", &cargo_home)
        .env("GITHUB_ENV", &env_file)
        .env("GITHUB_OUTPUT", &outputs);
    for (name, value) in &setup.env {
        command.env(name, value);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    let read = |file: &Path| fs::read_to_string(file).unwrap_or_default();
    Run {
        output,
        env_file: read(&env_file),
        outputs: read(&outputs),
        cargo_config: read(&cargo_home.join("config.toml")),
        velnor_config: read(&dir.join(".cargo/config.toml")),
    }
}

fn prepare(name: &str, setup: &Setup) -> Run {
    prepare_in(&scratch(name), setup)
}

fn digest(run: &Run) -> &str {
    run.outputs
        .strip_prefix("compat=")
        .and_then(|rest| rest.strip_suffix('\n'))
        .unwrap_or_default()
}

#[test]
fn prepare_sets_the_build_profile_and_writes_a_compat_digest_when_every_tool_is_at_its_pin() {
    let run = prepare("ok", &Setup::new(reported("1.99.1", Some("0.9.151"))));
    assert!(run.output.status.success(), "{:?}", run.output);
    assert_eq!(
        run.env_file,
        "RUSTUP_AUTO_INSTALL=0\nCARGO_INCREMENTAL=0\nCARGO_TERM_COLOR=always\nSOURCE_DATE_EPOCH=0\nCARGO_PROFILE_DEV_DEBUG=line-tables-only\nCARGO_PROFILE_TEST_DEBUG=line-tables-only\n"
    );
    assert_eq!(digest(&run).len(), 16, "{}", run.outputs);
    assert!(digest(&run).bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(run.cargo_config, "");
    assert!(
        run.velnor_config.ends_with(DEPENDENCY_DEBUG),
        "{}",
        run.velnor_config
    );
}

#[test]
fn compat_is_stable_and_follows_rustflags_and_the_debuginfo_setting() {
    let json = reported("1.99.1", Some("0.9.151"));
    let first = prepare("stable", &Setup::new(json.clone()));
    let again = prepare("stable", &Setup::new(json.clone()));
    assert_eq!(digest(&first), digest(&again));
    let mut flags = Setup::new(json.clone());
    flags.env.push(("RUSTFLAGS", "-C target-cpu=native"));
    assert_ne!(digest(&first), digest(&prepare("stable", &flags)));
    let mut full = Setup::new(json);
    full.rust_ci_toml = Some("ci_debuginfo = \"full\"\n");
    let full = prepare("stable", &full);
    assert_ne!(digest(&first), digest(&full));
    assert!(full.env_file.contains("CARGO_PROFILE_DEV_DEBUG=full\n"));
    assert_eq!(
        (full.cargo_config.as_str(), full.velnor_config.as_str()),
        ("", "")
    );
}

fn effective(dir: &Path) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .args(["-Zunstable-options", "config", "get", "profile.dev"])
        .current_dir(dir.join("checkout"))
        .env("CARGO_HOME", dir.join("cargo-home"))
        .env("RUSTC_BOOTSTRAP", "1")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn effective_text(dir: &Path) -> String {
    let output = effective(dir);
    if String::from_utf8_lossy(&output.stderr).contains("is not set") {
        return String::new();
    }
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

const DEBUG_NONE: &str = "profile.dev.package.\"*\".debug = \"none\"";

fn read_config(dir: &Path) -> String {
    fs::read_to_string(dir.join(".cargo/config.toml")).unwrap_or_default()
}

#[test]
fn a_user_config_that_already_has_the_override_table_still_loads_and_is_left_alone() {
    let dir = scratch("home");
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.cargo_config = Some("[profile.dev.package.\"*\"]\nopt-level = 1\n");
    let run = prepare_in(&dir, &setup);
    assert!(run.output.status.success(), "{:?}", run.output);
    assert_eq!(
        run.cargo_config,
        "[profile.dev.package.\"*\"]\nopt-level = 1\n"
    );
    let text = effective_text(&dir);
    assert!(
        text.contains(DEBUG_NONE) && text.contains("opt-level = 1"),
        "{text}"
    );
}

#[test]
fn prepares_in_a_row_and_at_once_leave_one_override() {
    let dir = scratch("twice");
    let setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    let first = prepare_in(&dir, &setup);
    let second = prepare_in(&dir, &setup);
    assert_eq!(second.velnor_config, first.velnor_config);
    assert_eq!(effective_text(&dir).matches(DEBUG_NONE).count(), 1);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| prepare_in(&dir, &setup));
        }
    });
    assert_eq!(effective_text(&dir).matches(DEBUG_NONE).count(), 1);
    assert_eq!(read_config(&dir), first.velnor_config);
}

#[test]
fn switching_the_debuginfo_setting_changes_the_effective_override() {
    let dir = scratch("switch");
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    prepare_in(&dir, &setup);
    assert!(effective_text(&dir).contains(DEBUG_NONE));
    setup.rust_ci_toml = Some("ci_debuginfo = \"full\"\n");
    prepare_in(&dir, &setup);
    assert!(!effective_text(&dir).contains(DEBUG_NONE));
    setup.rust_ci_toml = None;
    prepare_in(&dir, &setup);
    assert!(effective_text(&dir).contains(DEBUG_NONE));
}

#[test]
fn a_config_file_velnor_did_not_write_is_never_replaced() {
    let dir = scratch("foreign");
    fs::create_dir_all(dir.join(".cargo")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(dir.join(".cargo/config.toml"), "[net]\nretry = 2\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    let run = prepare_in(&dir, &Setup::new(reported("1.99.1", Some("0.9.151"))));
    assert_eq!(run.output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&run.output.stderr).contains(".cargo/config.toml"));
    assert_eq!(read_config(&dir), "[net]\nretry = 2\n");
}

#[test]
fn prepare_without_rustc_fails_and_writes_nothing() {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.rustc = false;
    let run = prepare("norustc", &setup);
    assert_eq!(run.output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&run.output.stderr).contains("rustc"));
    assert_eq!(
        (
            run.env_file.as_str(),
            run.outputs.as_str(),
            run.cargo_config.as_str()
        ),
        ("", "", "")
    );
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
        let run = prepare(name, &Setup::new(json));
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
