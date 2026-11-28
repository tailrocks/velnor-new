// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use anyhow::{Context, Result};

use crate::mise_double::{cancel_during_paths, each_row, git_bytes, prepared, private_dirs};
use crate::mise_repo::{REPO_MISE_LOCK, repo_config_without_wrappers};
use crate::mise_world::{CONFIG, World, git, job_fixture};

const CALLS: [&str; 5] = [
    "config ls --json",
    "install --locked",
    "env --redacted --json",
    "env --json",
    "env --paths",
];
const KEYS: [&str; 14] = [
    "PATH",
    "HOME",
    "MISE_CEILING_PATHS",
    "MISE_SYSTEM_CONFIG_DIR",
    "MISE_SYSTEM_DATA_DIR",
    "MISE_SYSTEM_PLUGINS_DIRS",
    "MISE_TRUSTED_CONFIG_PATHS",
    "MISE_DATA_DIR",
    "MISE_SHIMS_DIR",
    "MISE_CACHE_DIR",
    "MISE_AUTO_INSTALL",
    "MISE_YES",
    "MISE_CARGO_HOME",
    "MISE_RUSTUP_HOME",
];

#[test]
fn mise_runs_with_exactly_the_velnor_environment() -> Result<()> {
    let world = prepared()?;
    let job = world.with_job_env(
        &job_fixture("mise_action_ok"),
        &[
            ("MISE_ENV", "ci"),
            ("MISE_CD", "/nonexistent"),
            ("XDG_CONFIG_HOME", "/x"),
            ("MISE_SYSTEM_CONFIG_DIR", "/s"),
            ("FOO", "bar"),
        ],
    )?;
    let ran = world.run(
        &job,
        &[("MISE_CONFIG_DIR", "/p"), ("MISE_DOUBLE_LEAK", "1")],
    )?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    let calls = world.calls();
    let argv: Vec<&str> = calls.iter().map(|call| call.argv.as_str()).collect();
    assert_eq!(argv, CALLS);
    let root = calls[0].root();
    let system = root.join("system");
    let expected = [
        (
            "PATH",
            format!(
                "{}:/usr/bin:/bin:/usr/sbin:/sbin",
                world.binary_dir().display()
            ),
        ),
        ("HOME", format!("{}/home", root.display())),
        ("MISE_CEILING_PATHS", root.display().to_string()),
        ("MISE_SYSTEM_CONFIG_DIR", system.display().to_string()),
        ("MISE_SYSTEM_DATA_DIR", system.display().to_string()),
        (
            "MISE_SYSTEM_PLUGINS_DIRS",
            format!("{}/plugins", system.display()),
        ),
        (
            "MISE_TRUSTED_CONFIG_PATHS",
            format!("{}/config", root.display()),
        ),
        (
            "MISE_DATA_DIR",
            format!("{}/tools/data", world.store.display()),
        ),
        (
            "MISE_SHIMS_DIR",
            format!("{}/mise-shims", world.temp().display()),
        ),
        ("MISE_CACHE_DIR", format!("{}/cache", root.display())),
        ("MISE_AUTO_INSTALL", "false".to_owned()),
        ("MISE_YES", "1".to_owned()),
        (
            "MISE_CARGO_HOME",
            format!("{}/tools/cargo", world.store.display()),
        ),
        (
            "MISE_RUSTUP_HOME",
            format!("{}/tools/rustup", world.store.display()),
        ),
    ];
    for call in &calls {
        let keys: BTreeSet<&str> = call.env.keys().map(String::as_str).collect();
        let wanted: BTreeSet<&str> = KEYS.into_iter().collect();
        let leaked: Vec<&&str> = keys.difference(&wanted).collect();
        let dropped: Vec<&&str> = wanted.difference(&keys).collect();
        assert!(
            leaked.is_empty() && dropped.is_empty(),
            "{}: leaked {leaked:?}, dropped {dropped:?}",
            call.argv
        );
        assert_eq!(call.root(), root, "{}", call.argv);
        for (name, value) in &expected {
            assert_eq!(&call.env_value(name), value, "{} {name}", call.argv);
        }
    }
    Ok(())
}

#[test]
fn mise_runs_in_a_private_copy_of_the_accepted_config() -> Result<()> {
    each_row(&[false, true], |linked| {
        let mut world = World::new()?;
        if linked {
            world.symlinked_temp()?;
        }
        world.install_mise()?;
        let config = "# Привет, 日本語\n[settings]\nlockfile = true\n\n[tools]\nnode = \"26.13.1\"";
        world.init_workspace_with(config, true)?;
        let workspace = world.workspace();
        std::fs::write(
            workspace.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"stable\"\n",
        )?;
        git(&workspace, &["add", "rust-toolchain.toml"])?;
        git(&workspace, &["commit", "--quiet", "--message", "toolchain"])?;
        std::fs::write(workspace.join("mise.local.toml"), "[env]\nLOCAL = \"1\"\n")?;
        std::fs::write(workspace.join(".tool-versions"), "node 22.0.0\n")?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert!(ran.has_line("job succeeded"), "{linked}\n{}", ran.stdout);
        let calls = world.calls();
        let canonical_workspace = std::fs::canonicalize(&workspace)?;
        let canonical_temp = world.canonical_temp()?;
        for call in &calls {
            let label = format!("linked={linked} {}", call.argv);
            assert!(!call.pwd.starts_with(&canonical_workspace), "{label}");
            assert!(!call.pwd.starts_with(&workspace), "{label}");
            assert_eq!(call.pwd, call.root().join("config"), "{label}");
            assert!(
                call.root().parent() == Some(canonical_temp.as_path())
                    && call.root().to_string_lossy().contains("/velnor-mise-"),
                "{label}: {}",
                call.root().display()
            );
            assert_eq!(call.ls, ["mise.lock", "mise.toml"], "{label}");
            for name in ["mise.toml", "mise.lock"] {
                assert_eq!(
                    call.files.get(name),
                    Some(&git_bytes(&workspace, name)?),
                    "{label} {name}"
                );
            }
            assert_eq!(
                call.rootls,
                ["cache", "config", "home", "system"],
                "{label}"
            );
            assert!(
                call.rootmode.starts_with("drwx------"),
                "{label}: {}",
                call.rootmode
            );
            assert!(
                call.homels.is_empty() && call.systemls.is_empty(),
                "{label}"
            );
            assert_eq!(
                call.env_value("MISE_CEILING_PATHS"),
                call.root().display().to_string(),
                "{label}"
            );
        }
        Ok(())
    })
}

#[test]
fn mise_action_rewrites_private_paths_to_the_workspace() -> Result<()> {
    let world = prepared()?;
    world.configure_double(&[("PATHS_IN_COPY", "1")])?;
    let job = world.with_final_script(
        &job_fixture("mise_action_ok"),
        "echo \"foo=$FOO cfg=$CFG path=$PATH secret=$SECRET\"",
    )?;
    let ran = world.run(&job, &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    let workspace = world.workspace().display().to_string();
    let prefix = format!(
        "foo={workspace}/x cfg={workspace} path={workspace}/bin:{workspace}/tbin:{}/mise-shims:{}",
        world.temp().display(),
        world.binary_dir().display()
    );
    let line = ran
        .lines()
        .into_iter()
        .find(|line| line.starts_with("foo="))
        .context("the later step line")?;
    assert!(
        line.starts_with(&prefix) && line.ends_with(" secret=***"),
        "{prefix}\n{line}"
    );
    let root = world.calls()[0].root().display().to_string();
    assert!(!ran.stdout.contains(&root), "{}", ran.stdout);
    assert!(
        !ran.stdout.contains(&format!("{workspace}/secret-token")),
        "{}",
        ran.stdout
    );
    Ok(())
}

fn run_row(world: &World, row: &str) -> Result<String> {
    match row {
        "cancel" => return cancel_during_paths(world),
        "install failure" => world.configure_double(&[("INSTALL_EXIT", "1")])?,
        "last check failure" => {
            world.configure_double(&[("CONFIG_LS_EXTRA", "/elsewhere/mise.toml")])?;
        }
        _ => {}
    }
    Ok(world.run(&job_fixture("mise_action_ok"), &[])?.stdout)
}

#[test]
fn mise_action_removes_the_private_dir() -> Result<()> {
    each_row(
        &["success", "install failure", "last check failure", "cancel"],
        |row| {
            let world = prepared()?;
            let stdout = run_row(&world, row)?;
            let calls = world.calls();
            assert!(
                calls
                    .first()
                    .is_some_and(|call| call.rootmode.starts_with("drwx")),
                "{row}: {stdout}"
            );
            let root = calls[0].root();
            assert!(!root.exists(), "{row}: {} survived", root.display());
            assert_eq!(private_dirs(&world)?, Vec::<String>::new(), "{row}");
            assert_eq!(
                stdout.contains("job succeeded"),
                row == "success",
                "{row}\n{stdout}"
            );
            assert_eq!(
                stdout.lines().any(|line| line == "job cancelled"),
                row == "cancel",
                "{row}\n{stdout}"
            );
            Ok(())
        },
    )
}

#[test]
fn mise_action_rejects_unsupported_env_directives() -> Result<()> {
    let rejected = [
        ("[env]\n_.file = \".env\"\n", "_.file"),
        ("[env]\n_.source = \"x.sh\"\n", "_.source"),
        ("[env]\n_.python.venv = \".venv\"\n", "_.python"),
        ("[env._]\nfile = \".env\"\n", "_.file"),
        ("[[env]]\n_.file = \".env\"\n", "_.file"),
        ("[[env]]\n_.source = \"x.sh\"\n", "_.source"),
        ("[[env]]\n_.python.venv = \".venv\"\n", "_.python"),
    ];
    for (section, directive) in rejected {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace_with(&format!("{CONFIG}\n{section}"), true)?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        let expected = format!(
            "unsupported: mise policy: [env] {directive} is not supported by velnor-runner"
        );
        assert!(ran.has_line(&expected), "{section}\n{}", ran.stdout);
        assert!(
            ran.has_line("step 1 failed") && ran.code == Some(1),
            "{section}\n{}",
            ran.stdout
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{section}");
        assert_eq!(private_dirs(&world)?, Vec::<String>::new(), "{section}");
    }
    for section in [
        "[env]\n_.path = [\"bin\"]\n",
        "[[env]]\n_.path = [\"bin\"]\n",
    ] {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace_with(&format!("{CONFIG}\n{section}"), true)?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert!(ran.has_line("job succeeded"), "{section}\n{}", ran.stdout);
    }
    Ok(())
}

#[test]
fn mise_action_accepts_the_repository_mise_toml_settings() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace_with_lock(&repo_config_without_wrappers()?, REPO_MISE_LOCK)?;
    let workspace = world.workspace();
    std::fs::write(
        workspace.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"stable\"\n",
    )?;
    git(&workspace, &["add", "rust-toolchain.toml"])?;
    git(&workspace, &["commit", "--quiet", "--message", "toolchain"])?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    assert_eq!(world.calls()[0].ls, ["mise.lock", "mise.toml"]);
    Ok(())
}

#[test]
fn mise_action_fails_when_mise_loads_another_config() -> Result<()> {
    for mode in ["extra", "none", "other"] {
        let world = prepared()?;
        let other = world.workspace().join("mise.toml").display().to_string();
        match mode {
            "extra" => world.configure_double(&[("CONFIG_LS_EXTRA", "/elsewhere/mise.toml")])?,
            "none" => world.configure_double(&[("CONFIG_LS_NONE", "1")])?,
            _ => world.configure_double(&[("CONFIG_LS_ONLY", &other)])?,
        }
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert!(
            ran.lines()
                .iter()
                .any(|line| line.starts_with("unsupported: mise policy: mise loads [")),
            "{mode}\n{}",
            ran.stdout
        );
        assert!(
            ran.has_line("step 1 failed") && ran.code == Some(1),
            "{mode}\n{}",
            ran.stdout
        );
        assert_eq!(world.mise_calls(), vec!["argv=config ls --json"], "{mode}");
        assert!(!world.calls()[0].root().exists(), "{mode}");
    }
    Ok(())
}
