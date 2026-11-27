// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, finish, git};

pub(crate) fn real_git() -> Result<String> {
    let found = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .context("locate git")?;
    Ok(String::from_utf8_lossy(&found.stdout).trim().to_owned())
}

pub(crate) fn install_wrapper(root: &Path) -> Result<()> {
    let real = real_git()?;
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin)?;
    let wrapper = bin.join("git");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n[ -f '{root}/wrapper.env' ] && . '{root}/wrapper.env'\nGIT_TRACE2_EVENT='{root}/trace.json'\nexport GIT_TRACE2_EVENT\nexec '{real}' \"$@\"\n",
            root = root.display()
        ),
    )?;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

pub(crate) fn wrapper_env(root: &Path, script: &str) -> Result<()> {
    std::fs::write(root.join("wrapper.env"), script)?;
    Ok(())
}

#[test]
fn repository_selecting_variables_in_the_runner_environment_have_no_effect() -> Result<()> {
    let rig = Rig::new()?;
    let decoy = rig.root.join("decoy");
    std::fs::create_dir_all(&decoy)?;
    git(&decoy, &["init", "-q", "-b", "main"])?;
    let before = std::fs::read_dir(decoy.join(".git"))?.count();
    let decoy_git = decoy.join(".git");
    let objects = decoy_git.join("objects");
    let index = decoy_git.join("index");
    let env = [
        ("GIT_DIR", decoy_git.to_str().context("utf-8")?),
        ("GIT_OBJECT_DIRECTORY", objects.to_str().context("utf-8")?),
        ("GIT_INDEX_FILE", index.to_str().context("utf-8")?),
    ];
    let job = rig.job("checkout_depth1", &rig.head, &rig.server_url())?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start_env(&job, &scratch, &env)?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    assert!(
        rig.mirror().join("HEAD").exists(),
        "the mirror was not filled"
    );
    assert!(!index.exists(), "the decoy index was written");
    assert_eq!(std::fs::read_dir(&decoy_git)?.count(), before);
    let decoy_commits = Command::new("git")
        .args(["--git-dir"])
        .arg(&decoy_git)
        .args(["rev-list", "--all", "--count"])
        .output()?;
    assert_eq!(String::from_utf8_lossy(&decoy_commits.stdout).trim(), "0");
    Ok(())
}

const NETWORK_ENV: [&str; 11] = [
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "NO_PROXY",
    "no_proxy",
    "ALL_PROXY",
    "all_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "GIT_SSL_CAINFO",
];

#[test]
fn proxy_and_ca_variables_reach_the_broker_fetch_and_not_the_workspace_git() -> Result<()> {
    let rig = Rig::new()?;
    let seen = rig.root.join("seen");
    wrapper_env(
        &rig.root,
        &format!(
            "case \" $* \" in *\" fetch \"*) k=fetch;; *\" init \"*) k=init;; *\" remote \"*) k=remote;; *\" checkout-index \"*) k=checkout;; *) k=other;; esac\n\
             for v in {names}; do eval \"s=\\${{$v+set}}\"; if [ -n \"$s\" ]; then eval \"x=\\$$v\"; echo \"$k $v=$x\" >> '{seen}'; fi; done\n",
            names = NETWORK_ENV.join(" "),
            seen = seen.display()
        ),
    )?;
    let values: Vec<(&str, String)> = NETWORK_ENV
        .iter()
        .map(|name| (*name, format!("{name}-value")))
        .collect();
    let env: Vec<(&str, &str)> = values
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let job = rig.job("checkout_depth1", &rig.head, &rig.server_url())?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start_env(&job, &scratch, &env)?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    let mut recorded: Vec<String> = std::fs::read_to_string(&seen)?
        .lines()
        .map(str::to_owned)
        .collect();
    recorded.sort();
    let mut expected: Vec<String> = values
        .iter()
        .map(|(name, value)| format!("fetch {name}={value}"))
        .collect();
    expected.sort();
    assert_eq!(recorded, expected);
    Ok(())
}
