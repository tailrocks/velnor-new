// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use super::fetch::{Fixture, TOKEN, TestResult, script, serve};

const PROXY_CONFIG: &str = "[core]\n\tbare = true\n[http \"https://github.com/\"]\n\tproxy = http://127.0.0.1:9\n\tsslVerify = false\n";

fn cargo_writing_git(fixture: &Fixture) -> TestResult {
    let root = fixture.repo.root();
    script(
        &root.join("trusted/bin/cargo"),
        &format!(
            "pwd -P > {log}/cargo.pwd\necho \"$*\" > {log}/cargo.args\nenv > {log}/cargo.env\nls -A \"$CARGO_HOME/git\" > {log}/git.seen\n\
             db=\"$CARGO_HOME/git/db/dep-1\"\nmkdir -p \"$db/objects/pack\" \"$db/hooks\"\nprintf '{PROXY}' > \"$db/config\"\n\
             echo pack > \"$db/objects/pack/new.pack\"\necho hook > \"$db/hooks/post-checkout\"\n\
             mkdir -p \"$CARGO_HOME/git/db/old-1/objects/pack\"\necho fresh > \"$CARGO_HOME/git/db/old-1/objects/pack/fresh.pack\"\n\
             co=\"$CARGO_HOME/git/checkouts/dep-1/abc\"\nmkdir -p \"$co/.git/hooks\"\necho source > \"$co/lib.rs\"\ntouch \"$co/.cargo-ok\"\n\
             printf '[remote \"origin\"]\\n\\turl = /scratch/db\\n' > \"$co/.git/config\"\necho hook > \"$co/.git/hooks/post-merge\"",
            log = root.join("log").display(),
            PROXY = PROXY_CONFIG.replace('\n', "\\n").replace('\t', "\\t"),
        ),
    )?;
    Ok(())
}

fn existing_database(root: &Path, config: &str) -> TestResult {
    let database = root.join("home/.cargo/git/db/old-1");
    fs::create_dir_all(database.join("objects/pack"))?;
    fs::write(database.join("config"), config)?;
    fs::write(database.join("objects/pack/kept.pack"), "kept")?;
    Ok(())
}

#[test]
fn a_url_scoped_proxy_in_a_restored_git_database_refuses_the_fetch_and_still_revokes() -> TestResult
{
    let fixture = Fixture::new("fetch-git-proxy", 0)?;
    let root = fixture.repo.root().to_path_buf();
    existing_database(&root, PROXY_CONFIG)?;
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &[]);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("old-1"),
        "{output:?}"
    );
    assert_eq!(fixture.log("cargo.args"), "");
    assert_eq!(fixture.log("git.env"), "");
    assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    Ok(())
}

#[test]
fn the_token_fetch_starts_from_an_empty_git_directory_and_its_sources_join_the_cache_as_data()
-> TestResult {
    let fixture = Fixture::new("fetch-git-fresh", 0)?;
    let root = fixture.repo.root().to_path_buf();
    existing_database(&root, "[core]\n\tbare = true\n")?;
    cargo_writing_git(&fixture)?;
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.log("git.seen"), "");
    let git = root.join("home/.cargo/git");
    let read = |path: &str| fs::read_to_string(git.join(path)).unwrap_or_default();
    assert_eq!(read("db/dep-1/objects/pack/new.pack"), "pack\n");
    assert!(
        !read("db/dep-1/config").contains("proxy"),
        "{}",
        read("db/dep-1/config")
    );
    assert!(read("db/dep-1/config").contains("bare = true"));
    assert!(!git.join("db/dep-1/hooks").exists());
    assert_eq!(read("db/old-1/objects/pack/kept.pack"), "kept");
    assert_eq!(read("db/old-1/objects/pack/fresh.pack"), "fresh\n");
    assert_eq!(read("db/old-1/config"), "[core]\n\tbare = true\n");
    assert_eq!(read("checkouts/dep-1/abc/lib.rs"), "source\n");
    assert!(git.join("checkouts/dep-1/abc/.cargo-ok").is_file());
    assert!(!read("checkouts/dep-1/abc/.git/config").contains("scratch"));
    assert!(!git.join("checkouts/dep-1/abc/.git/hooks").exists());
    assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    Ok(())
}

#[test]
fn the_token_fetch_runs_outside_the_checkout_and_the_runner_home_with_an_absolute_manifest()
-> TestResult {
    let fixture = Fixture::new("fetch-git-cwd", 0)?;
    let root = fs::canonicalize(fixture.repo.root())?;
    fs::write(
        root.join("home/.cargo/config.toml"),
        "[source.crates-io]\nreplace-with = \"planted\"\n",
    )?;
    let runner_temp = root.join("home/work/_temp");
    fs::create_dir_all(&runner_temp)?;
    fixture.repo.write_unstaged("tools/Cargo.toml", "");
    let api = serve(204)?;
    let output = fixture.run_with(
        &api,
        Some(TOKEN),
        None,
        &["tools/Cargo.toml"],
        &[("RUNNER_TEMP", &runner_temp.display().to_string())],
    );
    assert!(output.status.success(), "{output:?}");
    let cwd = fixture.log("cargo.pwd");
    let cwd = Path::new(cwd.trim());
    assert!(!cwd.starts_with(&root), "{}", cwd.display());
    let environment = fixture.log("cargo.env");
    assert!(
        environment
            .lines()
            .any(|line| line == format!("CARGO_HOME={}", cwd.display())),
        "{environment}"
    );
    assert_eq!(
        fixture.log("cargo.args").trim(),
        format!(
            "fetch --locked --manifest-path {} --config net.offline=false",
            root.join("tools/Cargo.toml").display()
        )
    );
    Ok(())
}
