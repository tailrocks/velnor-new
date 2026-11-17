// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::io::Repo;

mod fixture;
mod mise;
mod setup;
mod toolchains;
use fixture::{
    TestResult, active_toolchain, mirror_fixture, repeated_mirror_fixture, rustup_binary,
};

#[test]
fn missing_rustup_fails_mirror_integration_prerequisite() -> TestResult {
    let repo = Repo::empty("crates-mirror-missing-rustup");
    fs::create_dir(repo.root().join("empty-path"))?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "crates_mirror::cargo_fetches_uncached_crates_with_changed_homes_and_explicit_toolchain",
            "--nocapture",
        ])
        .env("PATH", repo.root().join("empty-path"))
        .output()?;
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8(output.stderr)?.contains("rustup unavailable"),
        "fixture must report its missing Rustup prerequisite"
    );
    Ok(())
}

#[test]
fn cargo_fetches_uncached_crates_with_changed_homes_and_explicit_toolchain() -> TestResult {
    repeated_mirror_fixture(direct_fetches)
}

#[test]
fn build_scripts_fetch_uncached_crates_from_tmp_through_path_and_cargo_env() -> TestResult {
    mirror_fixture(child_fetches)
}

#[test]
fn relative_path_cargo_reaches_children_in_tmp() -> TestResult {
    mirror_fixture(|repo, wrapper, config| {
        let output = Command::new(rustup_binary().ok_or("rustup unavailable")?)
            .args(["which", "cargo"])
            .output()?;
        assert!(output.status.success(), "{output:?}");
        let cargo = String::from_utf8(output.stdout)?;
        fs::create_dir(repo.root().join("native"))?;
        std::os::unix::fs::symlink(cargo.trim(), repo.root().join("native/cargo"))?;
        child_fetches_with_path(repo, wrapper, config, Path::new("../native"), None)
    })
}

#[test]
fn nested_cargo_uses_saved_executable_when_relative_path_is_unavailable() -> TestResult {
    mirror_fixture(|repo, wrapper, config| {
        let output = Command::new(rustup_binary().ok_or("rustup unavailable")?)
            .args(["which", "cargo"])
            .output()?;
        assert!(output.status.success(), "{output:?}");
        let cargo = String::from_utf8(output.stdout)?;
        let cargo = Path::new(cargo.trim());
        let root = repo.root();
        fs::create_dir(root.join("native"))?;
        std::os::unix::fs::symlink(cargo, root.join("native/cargo"))?;
        child_projects(repo);
        let paths = [
            Path::new("../native"),
            wrapper.parent().ok_or("wrapper parent")?,
        ];
        let output = Command::new(wrapper)
            .args(["build", "--manifest-path"])
            .arg(root.join("outer/Cargo.toml"))
            .current_dir(root.join("outer"))
            .env_remove("RUSTUP_TOOLCHAIN")
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env_remove("MIRROR_EXPLICIT_TOOLCHAIN")
            .env_remove("VELNOR_CARGO_REAL")
            .env("RUSTC", cargo.with_file_name("rustc"))
            .env("RUSTFLAGS", "-Clinker=/usr/bin/cc")
            .env("MIRROR_FIXTURE_ROOT", root)
            .env("MIRROR_EXPECTED_NO_TOOLCHAIN", "1")
            .env("CARGO_HOME", root.join("outer-home"))
            .env("CARGO_TARGET_DIR", root.join("outer-target"))
            .env("VELNOR_CARGO_CONFIG", config)
            .env("VELNOR_CARGO_WRAPPER", wrapper)
            .env("PATH", std::env::join_paths(paths)?)
            .env("CARGO_NET_OFFLINE", "false")
            .env("CARGO_HTTP_PROXY", "http://127.0.0.1:1")
            .env("NO_PROXY", "127.0.0.1")
            .env("CARGO_NET_RETRY", "0")
            .env(
                "CARGO_REGISTRIES_CRATES_IO_INDEX",
                "sparse+http://127.0.0.1:1/index/",
            )
            .output()?;
        assert!(output.status.success(), "{output:?}");
        Ok(())
    })
}

fn direct_fetches(repo: &Repo, wrapper: &Path, config: &Path) -> TestResult {
    direct_fetches_using(repo, wrapper, config, "shadow")
}

fn direct_fetches_using(repo: &Repo, wrapper: &Path, config: &Path, proxy: &str) -> TestResult {
    let root = repo.root();
    for (index, program, toolchain) in [
        (0, "cargo", false),
        (1, wrapper.to_str().ok_or("wrapper path")?, true),
    ] {
        let project = format!("tmp-{index}");
        repo.write_unstaged(&format!("{project}/Cargo.toml"), "[package]\nname='consumer'\nversion='1.0.0'\nedition='2024'\n[dependencies]\nprobe='=1.0.0'\n");
        repo.write_unstaged(&format!("{project}/src/lib.rs"), "");
        let paths = [
            fs::canonicalize(root.join("velnor-cargo"))?,
            root.join(proxy),
            std::path::PathBuf::from(std::env::var_os("HOME").ok_or("HOME")?).join(".cargo/bin"),
        ]
        .into_iter()
        .chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ))
        .collect::<Vec<_>>();
        let mut command = Command::new(program);
        if toolchain {
            command.env("VELNOR_CARGO_REAL", "/usr/bin/false");
            command.arg(format!("+{}", active_toolchain()?));
        }
        let output = command
            .args(["fetch", "--manifest-path"])
            .arg(root.join(&project).join("Cargo.toml"))
            .current_dir(root.join(&project))
            .env("PATH", std::env::join_paths(paths)?)
            .env("CARGO_HOME", root.join(format!("different-home-{index}")))
            .env("VELNOR_CARGO_CONFIG", config)
            .env("VELNOR_CARGO_WRAPPER", wrapper)
            .env("CARGO_NET_OFFLINE", "false")
            .env("CARGO_HTTP_PROXY", "http://127.0.0.1:1")
            .env("NO_PROXY", "127.0.0.1")
            .env("CARGO_NET_RETRY", "0")
            .env(
                "CARGO_REGISTRIES_CRATES_IO_INDEX",
                "sparse+http://127.0.0.1:1/index/",
            )
            .output()?;
        assert!(output.status.success(), "{output:?}");
        assert!(
            root.join(format!("different-home-{index}/registry/src"))
                .is_dir()
        );
    }
    Ok(())
}

fn child_fetches(repo: &Repo, wrapper: &Path, config: &Path) -> TestResult {
    fs::create_dir(repo.root().join("native"))?;
    std::os::unix::fs::symlink(
        Path::new(&active_toolchain()?).join("bin/cargo"),
        repo.root().join("native/cargo"),
    )?;
    child_fetches_using(repo, wrapper, config, "native", None)
}

fn child_projects(repo: &Repo) {
    repo.write_unstaged("tmp-0/Cargo.toml", "[package]\nname='consumer'\nversion='1.0.0'\nedition='2024'\n[dependencies]\nprobe='=1.0.0'\n");
    repo.write_unstaged("tmp-0/src/lib.rs", "");
    repo.write_unstaged(
        "outer/Cargo.toml",
        "[package]\nname='outer'\nversion='1.0.0'\nedition='2024'\n",
    );
    repo.write_unstaged("outer/src/lib.rs", "");
    repo.write_unstaged("outer/build.rs", r#"fn main() {
        let root = std::env::var("MIRROR_FIXTURE_ROOT").unwrap();
        for (name, program) in [("path", "cargo".to_owned()), ("env", std::env::var("CARGO").unwrap())] {
            if std::env::var_os("MIRROR_EXPECTED_NO_TOOLCHAIN").is_some() {
                assert!(std::env::var_os("RUSTUP_TOOLCHAIN").is_none());
            }
            if let Ok(expected) = std::env::var("MIRROR_EXPECTED_TOOLCHAIN") {
                assert_eq!(std::env::var("RUSTUP_TOOLCHAIN").unwrap(), expected);
                let selected = std::process::Command::new(std::env::var_os("MIRROR_RUSTUP").unwrap()).args(["show", "active-toolchain"])
                    .current_dir(std::env::temp_dir()).output().unwrap();
                assert!(selected.status.success());
                assert_eq!(String::from_utf8(selected.stdout).unwrap(), format!("{expected} (overridden by environment variable RUSTUP_TOOLCHAIN)\n"));
            }
            let mut command = std::process::Command::new(program);
            if name == "env" {
                if let Ok(toolchain) = std::env::var("MIRROR_EXPLICIT_TOOLCHAIN") {
                    command.arg(format!("+{toolchain}"));
                }
            }
            let status = command.args(["fetch", "--manifest-path"])
                .arg(format!("{root}/tmp-0/Cargo.toml"))
                .env("CARGO_HOME", format!("{root}/child-home-{name}"))
                .current_dir(std::env::temp_dir()).status().unwrap();
            assert!(status.success());
        }
    }"#);
}

fn child_fetches_using(
    repo: &Repo,
    wrapper: &Path,
    config: &Path,
    proxy: &str,
    rustup: Option<(&Path, &str)>,
) -> TestResult {
    child_fetches_with_path(repo, wrapper, config, &repo.root().join(proxy), rustup)
}

fn child_fetches_with_path(
    repo: &Repo,
    wrapper: &Path,
    config: &Path,
    proxy: &Path,
    rustup: Option<(&Path, &str)>,
) -> TestResult {
    child_projects(repo);
    let root = repo.root();
    let paths = [
        proxy.to_path_buf(),
        wrapper.parent().ok_or("wrapper parent")?.to_path_buf(),
        root.join("shadow"),
        std::path::PathBuf::from(std::env::var_os("HOME").ok_or("HOME")?).join(".cargo/bin"),
    ]
    .into_iter()
    .chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ))
    .collect::<Vec<_>>();
    let mut command = Command::new(wrapper);
    if let Some((home, expected)) = rustup {
        let expected = if Path::new(expected).is_absolute() {
            std::path::PathBuf::from(expected)
        } else {
            home.join("toolchains").join(expected)
        };
        command
            .env_remove("RUSTUP_TOOLCHAIN")
            .env("RUSTUP_HOME", home)
            .env("MIRROR_EXPECTED_TOOLCHAIN", expected);
    } else {
        command
            .env_remove("RUSTUP_TOOLCHAIN")
            .env("MIRROR_EXPECTED_NO_TOOLCHAIN", "1")
            .env("MIRROR_EXPLICIT_TOOLCHAIN", active_toolchain()?);
    }
    let output = command
        .args(["build", "--manifest-path"])
        .current_dir(root.join("outer"))
        .arg(root.join("outer/Cargo.toml"))
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env("MIRROR_FIXTURE_ROOT", root)
        .env(
            "MIRROR_RUSTUP",
            rustup_binary().ok_or("rustup unavailable")?,
        )
        .env("CARGO_HOME", root.join("outer-home"))
        .env("CARGO_TARGET_DIR", root.join("outer-target"))
        .env("VELNOR_CARGO_CONFIG", config)
        .env("VELNOR_CARGO_WRAPPER", wrapper)
        .env("PATH", std::env::join_paths(paths)?)
        .env("CARGO_NET_OFFLINE", "false")
        .env("CARGO_HTTP_PROXY", "http://127.0.0.1:1")
        .env("NO_PROXY", "127.0.0.1")
        .env("CARGO_NET_RETRY", "0")
        .env(
            "CARGO_REGISTRIES_CRATES_IO_INDEX",
            "sparse+http://127.0.0.1:1/index/",
        )
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(())
}
