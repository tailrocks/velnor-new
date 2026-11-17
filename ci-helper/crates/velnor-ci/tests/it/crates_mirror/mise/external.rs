// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
fn main() {
    let name = std::env::args().next().unwrap();
    let name = std::path::Path::new(&name).file_name().unwrap();
    if name == "cargo-activation" {
        let root = std::env::var("CONFIG_ROOT_MARKER").unwrap();
        for (name, directory) in [("CARGO_HOME", "cargo-home"), ("RUSTUP_HOME", "rustup-home")] {
            assert_eq!(
                std::fs::canonicalize(std::env::var(name).unwrap()).unwrap(),
                std::fs::canonicalize(format!("{root}/{directory}")).unwrap()
            );
        }
        std::fs::write(format!("{root}/native-activation"), "activated").unwrap();
        return;
    }
    if name == "fixture-other-command" {
        std::fs::write(
            format!(
                "{}/other-called",
                std::env::var("CONFIG_ROOT_MARKER").unwrap()
            ),
            "called",
        )
        .unwrap();
        return;
    }
    if name == "cargo-fixture" {
        if let Ok(root) = std::env::var("MIRROR_FIXTURE_ROOT") {
            assert_eq!(
                std::fs::canonicalize(std::env::var("CONFIG_ROOT_MARKER").unwrap()).unwrap(),
                std::fs::canonicalize(&root).unwrap()
            );
            assert_eq!(
                std::fs::canonicalize(std::env::var("CARGO").unwrap()).unwrap(),
                std::fs::canonicalize(std::env::var("VELNOR_CARGO_WRAPPER").unwrap()).unwrap()
            );
            assert!(
                std::path::Path::new(&std::env::var("VELNOR_CARGO_REAL").unwrap()).is_absolute()
            );
            for (name, program) in [
                ("path", "cargo".to_owned()),
                ("env", std::env::var("CARGO").unwrap()),
            ] {
                let status = std::process::Command::new(program)
                    .args(["fetch", "--manifest-path"])
                    .arg(format!("{root}/tmp-0/Cargo.toml"))
                    .current_dir(std::env::temp_dir())
                    .env("CARGO_HOME", format!("{root}/child-home-{name}"))
                    .status()
                    .unwrap();
                assert!(status.success());
            }
            return;
        }
        let status = std::process::Command::new("fixture-other")
            .status()
            .unwrap();
        std::process::exit(status.code().unwrap_or(1));
    }
    let implicit = std::path::Path::new(&std::env::args().next().unwrap())
        .file_name()
        .unwrap()
        == "mbx";
    let program = if implicit || std::env::var_os("EXTERNAL_MISE_ENTRY").is_some() {
        if implicit {
            assert_eq!(std::env::var("MBX_CARGO_SHIM_MODE").unwrap(), "1");
        }
        let cargo = std::env::var("CARGO").unwrap();
        assert!(std::path::Path::new(&cargo).is_absolute());
        assert!(!cargo.contains("command-wrappers"));
        cargo
    } else {
        assert_eq!(std::env::var("WRAPPER_MARKER").unwrap(), "preserved");
        "cargo".to_owned()
    };
    let root = std::env::var("CONFIG_ROOT_MARKER").unwrap();
    assert_eq!(
        std::fs::canonicalize(std::env::var("MISE_DATA_DIR").unwrap()).unwrap(),
        std::fs::canonicalize(format!("{root}/mise-data")).unwrap()
    );
    for (name, directory) in [("CARGO_HOME", "cargo-home"), ("RUSTUP_HOME", "rustup-home")] {
        assert_eq!(
            std::fs::canonicalize(std::env::var(name).unwrap()).unwrap(),
            std::fs::canonicalize(format!("{root}/{directory}")).unwrap()
        );
    }
    let mut calls = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{root}/wrapper-calls"))
        .unwrap();
    writeln!(calls, "cargo").unwrap();
    let status = std::process::Command::new(program)
        .args(std::env::args_os().skip(1))
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}
