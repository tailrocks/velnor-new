use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use velnor_actions_contract::{GeneratorReleaseTarget, StepKind};

use super::super::{generator_release_mise_binary_sha256, mbx_cargo_build_step, setup_rust_steps};
use super::{catalog, strings};
use crate::catalog::{MR_BOXINGTON_VERSION, RUST_VERSION, ToolCatalog};
use crate::steps::ToolHomes;

const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";

#[test]
fn setup_installs_exact_rust_and_mbx_pins() {
    let catalog = ToolCatalog::pinned();
    let homes = ToolHomes::runner_temp();
    let target = GeneratorReleaseTarget::LinuxX86_64;
    let steps = setup_rust_steps(
        MISE_USES,
        target,
        generator_release_mise_binary_sha256(target),
        &homes,
        &catalog,
    )
    .expect("the exact target setup must be valid");

    assert_eq!(steps.len(), 2);
    let StepKind::Shell { run, env } = &steps[1].kind else {
        panic!("the second setup step must install tools");
    };
    let expected_run = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "install",
        "rust@1.98.1",
        "mr-boxington@1.21.1",
    ]
    .map(str::to_owned)
    .to_vec();
    assert_eq!(run, &expected_run,);
    assert_eq!(
        env,
        &BTreeMap::from([
            ("MISE_CARGO_HOME".to_owned(), homes.cargo_home().to_owned()),
            ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
            ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
            ("MISE_NO_ENV".to_owned(), "1".to_owned()),
            ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
            (
                "MISE_RUSTUP_HOME".to_owned(),
                homes.rustup_home().to_owned(),
            ),
            ("RUSTUP_TOOLCHAIN".to_owned(), RUST_VERSION.to_owned()),
        ])
    );
    assert!(run.iter().any(|arg| arg == &format!("rust@{RUST_VERSION}")));
    assert!(
        run.iter()
            .any(|arg| arg == &format!("mr-boxington@{MR_BOXINGTON_VERSION}"))
    );
}

#[test]
fn macos_x64_setup_uses_the_pinned_x64_mise_binary() {
    let target = GeneratorReleaseTarget::MacosX86_64;
    let steps = setup_rust_steps(
        MISE_USES,
        target,
        generator_release_mise_binary_sha256(target),
        &ToolHomes::runner_temp(),
        &ToolCatalog::pinned(),
    )
    .expect("the Intel runner must use its exact Mise pin");
    let StepKind::Action { with, .. } = &steps[0].kind else {
        panic!("the first setup step must install the pinned Mise action");
    };
    assert_eq!(
        with.get("sha256").map(String::as_str),
        Some("02d8ba561847f996925e361262c0610a24f59fcd9e06ba9ed0b6022e19b317c3")
    );
    assert!(
        setup_rust_steps(
            MISE_USES,
            target,
            "484c135bd4329975d608d3f77e26c2ece5d2f5590f18ca71f44440294f8cfa6f",
            &ToolHomes::runner_temp(),
            &ToolCatalog::pinned(),
        )
        .is_err()
    );
}

#[test]
fn mbx_build_uses_only_catalog_pins_and_owned_environment() {
    let catalog = catalog();
    let homes = ToolHomes::new("/runner/rustup", "/runner/cargo")
        .expect("test tool homes must be nonempty");
    let args = strings(&[
        "build",
        "--release",
        "--locked",
        "--target",
        "x86_64-unknown-linux-gnu",
        "--manifest-path",
        "crates/velnor-actions/Cargo.toml",
    ]);
    let step = mbx_cargo_build_step(
        "Build release generator",
        OsStr::new("cargo"),
        &args,
        &homes,
        &catalog,
    )
    .expect("a Cargo build request must lower to MBX");

    let StepKind::Shell { run, env } = step.kind else {
        panic!("the build adapter must emit a shell step");
    };
    assert_eq!(
        run,
        [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.97.0",
            "mr-boxington@1.20.0",
            "--",
            "mbx",
            "build",
            "--release",
            "--locked",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--manifest-path",
            "crates/velnor-actions/Cargo.toml",
        ]
    );
    assert_eq!(
        env,
        BTreeMap::from([
            ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
            ("MISE_CARGO_HOME".to_owned(), "/runner/cargo".to_owned()),
            ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
            ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
            ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
            ("MISE_NO_ENV".to_owned(), "1".to_owned()),
            ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
            ("MISE_RUSTUP_HOME".to_owned(), "/runner/rustup".to_owned()),
            ("RUSTUP_TOOLCHAIN".to_owned(), "1.97.0".to_owned()),
        ])
    );
}

#[test]
fn mbx_build_rejects_non_cargo_non_build_and_malformed_payloads() {
    let catalog = catalog();
    let homes = ToolHomes::runner_temp();
    let valid = strings(&["build", "--locked"]);

    assert!(mbx_cargo_build_step("Build", OsStr::new("mbx"), &valid, &homes, &catalog,).is_err());
    assert!(
        mbx_cargo_build_step(
            "Build",
            OsStr::new("/usr/bin/cargo"),
            &valid,
            &homes,
            &catalog,
        )
        .is_err()
    );
    assert!(
        mbx_cargo_build_step(
            "Build",
            OsStr::new("cargo"),
            &strings(&["metadata", "--no-deps"]),
            &homes,
            &catalog,
        )
        .is_err()
    );
    assert!(mbx_cargo_build_step("Build", OsStr::new("cargo"), &[], &homes, &catalog,).is_err());
    assert!(
        mbx_cargo_build_step(
            "Build",
            OsStr::new("cargo"),
            &strings(&["build", ""]),
            &homes,
            &catalog,
        )
        .is_err()
    );
    assert!(
        mbx_cargo_build_step(
            "Build",
            OsStr::new("cargo"),
            &strings(&["build", "--release\n"]),
            &homes,
            &catalog,
        )
        .is_err()
    );
    assert!(
        mbx_cargo_build_step("\nBuild", OsStr::new("cargo"), &valid, &homes, &catalog,).is_err()
    );
}

#[cfg(unix)]
#[test]
fn mbx_build_rejects_non_utf8_arguments() {
    use std::os::unix::ffi::OsStringExt;

    let catalog = catalog();
    let homes = ToolHomes::runner_temp();
    let args = [OsString::from("build"), OsString::from_vec(vec![0xff])];
    assert!(mbx_cargo_build_step("Build", OsStr::new("cargo"), &args, &homes, &catalog,).is_err());
}
