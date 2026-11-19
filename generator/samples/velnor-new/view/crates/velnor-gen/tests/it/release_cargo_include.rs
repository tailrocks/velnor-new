// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use crate::golden::samples;
use crate::release_symlink::{Scratch, copy_tree, git, run};

type TestResult = Result<(), Box<dyn Error>>;

const STEAL: &str = "[source.crates-io]\nreplace-with = \"mirror\"\n[registries.mirror]\nindex = \"sparse+http://127.0.0.1:1/\"\ncredential-provider = [\"./steal\"]\n";

fn repo(name: &str, config: &str) -> Result<Scratch, Box<dyn Error>> {
    let repo = Scratch(
        std::env::temp_dir().join(format!("velnor-gen-cargo-{name}-{}", std::process::id())),
    );
    fs::remove_dir_all(&repo.0).ok();
    copy_tree(&samples().join("release-two-roots/view"), &repo.0)?;
    fs::create_dir_all(repo.0.join(".cargo"))?;
    fs::create_dir_all(repo.0.join("cfg"))?;
    fs::write(repo.0.join(".cargo/config.toml"), config)?;
    fs::write(repo.0.join("cfg/extra.toml"), STEAL)?;
    git(&repo.0, &["init", "--quiet"])?;
    git(&repo.0, &["add", "--all"])?;
    Ok(repo)
}

fn refused(name: &str, config: &str, needle: &str) -> TestResult {
    let repo = repo(name, config)?;
    let output = run(&repo.0)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{config}: {stderr}");
    assert!(stderr.contains(".cargo/config.toml"), "{stderr}");
    assert!(stderr.contains(needle), "{config}: {stderr}");
    Ok(())
}

#[test]
fn a_cargo_config_include_is_refused_in_every_form_in_a_real_repository() -> TestResult {
    for (index, config) in [
        "include = \"../cfg/extra.toml\"\n",
        "include = [\"../cfg/extra.toml\"]\n",
        "include = [{ path = \"../cfg/extra.toml\", optional = true }]\n",
        "[[include]]\npath = \"../cfg/extra.toml\"\n",
        "Include = [\"../cfg/extra.toml\"]\n",
        "INCLUDE = [\"../cfg/extra.toml\"]\n",
        "include = [\"../cfg/extra.toml\"]\n\n[build]\njobs = 2\n\n[net]\nretry = 1\n",
    ]
    .into_iter()
    .enumerate()
    {
        refused(
            &format!("include-{index}"),
            config,
            "a cargo config include",
        )?;
    }
    Ok(())
}

#[test]
fn a_cargo_config_key_off_the_harmless_list_is_refused_in_a_real_repository() -> TestResult {
    for (index, config) in [
        "[alias]\nb = \"build\"\n",
        "[target.x86_64-unknown-linux-gnu]\nlinker = \"./steal\"\n",
        "[doc]\nbrowser = \"./steal\"\n",
        "[net]\ngit-fetch-with-cli = true\n",
        "[build]\nrustflags = [\"-Clinker=./steal\"]\n",
        "[registries.mirror]\ntoken = \"x\"\n",
        "[net]\ninclude = [\"../cfg/extra.toml\"]\n",
        "[future_key]\nanything = 1\n",
    ]
    .into_iter()
    .enumerate()
    {
        refused(&format!("unknown-{index}"), config, "not on the list")?;
    }
    Ok(())
}

#[test]
fn a_cargo_config_of_harmless_keys_is_accepted_in_a_real_repository() -> TestResult {
    let config = "[build]\ntarget-dir = \"out\"\njobs = 2\n\n[net]\nretry = 3\n\n[term]\ncolor = \"never\"\n\n[profile.release]\nlto = true\n\n[registries.mirror]\nindex = \"sparse+https://example.org/index/\"\n";
    let repo = repo("harmless", config)?;
    let output = run(&repo.0)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
