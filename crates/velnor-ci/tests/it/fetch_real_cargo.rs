// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use super::fetch::{Fixture, TOKEN, TestResult, script, serve};

/// Lays out what a repository's `[wrappers.cargo]` leaves on the runner: a command wrapper
/// and a shim for every name, each a copy of or link to `mise`, which refuses an untrusted
/// config and records that it ran.
fn mise_fronts(fixture: &Fixture, names: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let root = fixture.repo.root();
    let log = root.join("log/mise.ran");
    let trusted = root.join("trusted");
    let mise = trusted.join("mise-bin/mise");
    script(
        &mise,
        &format!(
            "echo \"$0\" >> {}\necho 'Config files in mise.toml are not trusted' >&2\nexit 1",
            log.display()
        ),
    )?;
    fs::create_dir_all(trusted.join("shims"))?;
    for name in names {
        symlink(&mise, trusted.join("shims").join(name))?;
        let wrapper = trusted.join("command-wrappers/bin").join(name);
        fs::create_dir_all(wrapper.parent().ok_or("parent")?)?;
        fs::copy(&mise, &wrapper)?;
    }
    Ok(format!(
        "{}:{}",
        trusted.join("command-wrappers/bin").display(),
        trusted.join("shims").display()
    ))
}

fn path_with(fronts: &str, fixture: &Fixture, tail: &str) -> String {
    let trusted = fixture.repo.root().join("trusted/bin");
    format!("{fronts}:{}:{tail}", trusted.display())
}

#[test]
fn a_mise_wrapper_and_shim_ahead_on_the_path_never_run_and_the_real_cargo_does() -> TestResult {
    let fixture = Fixture::new("fetch-real-cargo", 0)?;
    let fronts = mise_fronts(&fixture, &["cargo"])?;
    let path = path_with(&fronts, &fixture, "/usr/bin:/bin");
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &[("PATH", &path)]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.log("mise.ran"), "", "a mise front held the token");
    assert_eq!(
        fixture.log("cargo.args").trim(),
        "fetch --locked --manifest-path Cargo.toml --config net.offline=false"
    );
    let environment = fixture.log("cargo.env");
    let search = environment
        .lines()
        .find_map(|line| line.strip_prefix("PATH="))
        .ok_or("no PATH")?;
    assert!(
        !search.contains("shims") && !search.contains("command-wrappers"),
        "{search}"
    );
    Ok(())
}

#[test]
fn with_only_mise_fronts_for_cargo_the_fetch_refuses_clearly_and_still_revokes() -> TestResult {
    let fixture = Fixture::new("fetch-no-real-cargo", 0)?;
    fs::remove_file(fixture.repo.root().join("trusted/bin/cargo"))?;
    let fronts = mise_fronts(&fixture, &["cargo"])?;
    let path = format!("{fronts}:/usr/bin:/bin");
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &[("PATH", &path)]);
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no real cargo"), "{stderr}");
    assert!(!stderr.contains("not trusted"), "{stderr}");
    assert_eq!(fixture.log("mise.ran"), "");
    assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    Ok(())
}

#[test]
fn a_mise_shim_for_git_is_skipped_and_the_real_git_gets_the_token() -> TestResult {
    let fixture = Fixture::new("fetch-real-git", 0)?;
    let root = fixture.repo.root().to_path_buf();
    fs::rename(root.join("trusted/bin/git"), {
        fs::create_dir_all(root.join("trusted/gitbin"))?;
        root.join("trusted/gitbin/git")
    })?;
    let fronts = mise_fronts(&fixture, &["git"])?;
    let tail = format!("{}:/usr/bin:/bin", root.join("trusted/gitbin").display());
    let path = path_with(&fronts, &fixture, &tail);
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &[("PATH", &path)]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.log("mise.ran"), "");
    assert!(fixture.log("git.env").contains(TOKEN));
    Ok(())
}

fn toolchain_cargo(root: &Path, log: &Path) -> TestResult {
    script(
        &root.join("trusted/rustup/toolchains/stable-test/bin/cargo"),
        &format!(
            "env > {log}/cargo.env\necho \"$*\" > {log}/cargo.args",
            log = log.display()
        ),
    )
}

#[test]
fn a_rustup_proxy_resolves_to_the_toolchain_cargo_which_runs_in_place_of_the_proxy() -> TestResult {
    let fixture = Fixture::new("fetch-rustup-which", 0)?;
    let root = fixture.repo.root().to_path_buf();
    let log = root.join("log");
    let bin = root.join("trusted/bin");
    fs::remove_file(bin.join("cargo"))?;
    toolchain_cargo(&root, &log)?;
    let home = fs::canonicalize(root.join("trusted/rustup"))?;
    script(
        &bin.join("rustup"),
        &format!(
            "case \"$0\" in *rustup) case \"$1\" in which) echo {home}/toolchains/stable-test/bin/cargo ;; *) echo 'stable-test (default)' ;; esac ;; *) echo proxy >> {log}/proxy.ran ;; esac",
            home = home.display(),
            log = log.display()
        ),
    )?;
    symlink("rustup", bin.join("cargo"))?;
    let api = serve(204)?;
    let output = fixture.run_with(
        &api,
        Some(TOKEN),
        None,
        &["Cargo.toml"],
        &[("RUSTUP_HOME", &home.display().to_string())],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.log("proxy.ran"), "");
    let environment = fixture.log("cargo.env");
    let search = environment
        .lines()
        .find_map(|line| line.strip_prefix("PATH="))
        .ok_or("no PATH")?;
    assert!(
        search.starts_with(&format!("{}/toolchains/stable-test/bin", home.display())),
        "{search}"
    );
    assert!(environment.contains("RUSTUP_TOOLCHAIN=stable-test"));
    Ok(())
}
