// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;

use super::fetch::{Fixture, TOKEN, TestResult, script, serve};

type Place = fn(&Path, &Path) -> std::io::Result<()>;

fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::copy(from, to).map(drop)
}

fn hard_link(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::hard_link(from, to)
}

fn planted_above_runner_temp(fixture: &Fixture) -> Result<String, Box<dyn std::error::Error>> {
    let root = fixture.repo.root();
    let runner = root.join("runner");
    let runner_temp = runner.join("temp");
    fs::create_dir_all(&runner_temp)?;
    script(
        &root.join("evil/bin/cargo"),
        &format!("env > {}/evil.env", root.join("log").display()),
    )?;
    fs::write(
        runner.join("rust-toolchain.toml"),
        format!("[toolchain]\npath = \"{}\"\n", root.join("evil").display()),
    )?;
    Ok(runner_temp.display().to_string())
}

fn proxy(fixture: &Fixture) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let root = fixture.repo.root();
    let source = root.join("proxy/rustup");
    script(
        &source,
        &format!(
            "echo \"$RUSTUP_TOOLCHAIN\" >> {log}/proxy.ran\n\
             if [ -n \"$RUSTUP_TOOLCHAIN\" ]; then\n\
             chosen=\"$RUSTUP_HOME/toolchains/$RUSTUP_TOOLCHAIN/bin/cargo\"\n\
             [ -x \"$chosen\" ] && exec \"$chosen\" \"$@\"\n\
             echo \"error: toolchain '$RUSTUP_TOOLCHAIN' is not installed\" >&2\nexit 1\nfi\n\
             dir=$(pwd -P)\nwhile [ \"$dir\" != / ]; do\n\
             if [ -f \"$dir/rust-toolchain.toml\" ]; then\n\
             planted=$(sed -n 's/^path = \"\\(.*\\)\"$/\\1/p' \"$dir/rust-toolchain.toml\")\n\
             exec \"$planted/bin/cargo\" \"$@\"\nfi\ndir=$(dirname \"$dir\")\ndone\n\
             echo 'error: no default toolchain' >&2\nexit 1",
            log = root.join("log").display()
        ),
    )?;
    Ok(source)
}

#[test]
fn an_unpinned_linked_or_copied_proxy_never_follows_a_toolchain_file_above_the_scratch_home()
-> TestResult {
    let places: [(&str, Place); 2] = [("hard-link", hard_link), ("copy", copy)];
    for (label, place) in places {
        let fixture = Fixture::new(&format!("fetch-unpinned-{label}"), 0)?;
        let root = fixture.repo.root().to_path_buf();
        let runner_temp = planted_above_runner_temp(&fixture)?;
        let cargo = root.join("trusted/bin/cargo");
        fs::remove_file(&cargo)?;
        place(&proxy(&fixture)?, &cargo)?;
        assert!(!root.join("trusted/bin/rustup").exists());
        let api = serve(204)?;
        let output = fixture.run_with(
            &api,
            Some(TOKEN),
            None,
            &["Cargo.toml"],
            &[("RUNNER_TEMP", &runner_temp)],
        );
        assert_eq!(
            fixture.log("evil.env"),
            "",
            "{label}: the planted cargo ran"
        );
        assert!(!output.status.success(), "{label}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("is not installed"),
            "{label}: {output:?}"
        );
        let ran = fixture.log("proxy.ran");
        assert!(ran.starts_with("velnor-no-toolchain-"), "{label}: {ran}");
        assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    }
    Ok(())
}

#[test]
fn an_unpinned_real_cargo_still_fetches_with_a_toolchain_file_above_the_scratch_home() -> TestResult
{
    let fixture = Fixture::new("fetch-unpinned-real", 0)?;
    let runner_temp = planted_above_runner_temp(&fixture)?;
    let api = serve(204)?;
    let output = fixture.run_with(
        &api,
        Some(TOKEN),
        None,
        &["Cargo.toml"],
        &[("RUNNER_TEMP", &runner_temp)],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.log("evil.env"), "");
    let environment = fixture.log("cargo.env");
    assert!(environment.contains(TOKEN), "{environment}");
    assert!(
        environment
            .lines()
            .any(|line| line.starts_with("RUSTUP_TOOLCHAIN=velnor-no-toolchain-")),
        "{environment}"
    );
    assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    Ok(())
}

#[test]
fn git_under_the_token_refuses_a_local_repository_as_a_remote() -> TestResult {
    let fixture = Fixture::new("fetch-file-remote", 0)?;
    let root = fs::canonicalize(fixture.repo.root())?;
    let upstream = root.join("upstream.git");
    let created = Command::new("/usr/bin/git")
        .args(["init", "-q", "--bare"])
        .arg(&upstream)
        .status()?;
    assert!(created.success());
    let out = root.join("log/remote.out");
    script(
        &root.join("trusted/bin/cargo"),
        &format!(
            "env > {log}/cargo.env\nfor remote in file://{up} {up}; do /usr/bin/git ls-remote \"$remote\" >> {out} 2>&1; echo \"status=$?\" >> {out}; done",
            up = upstream.display(),
            out = out.display(),
            log = root.join("log").display()
        ),
    )?;
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &[]);
    assert!(output.status.success(), "{output:?}");
    let seen = fixture.log("remote.out");
    assert_eq!(
        seen.matches("transport 'file' not allowed").count(),
        2,
        "{seen}"
    );
    assert!(!seen.contains("status=0"), "{seen}");
    assert!(
        fixture
            .log("cargo.env")
            .lines()
            .any(|line| line == "CARGO_NET_GIT_FETCH_WITH_CLI=true")
    );
    assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    Ok(())
}
