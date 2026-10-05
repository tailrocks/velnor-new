use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::Path;

use velnor_actions_contract::{GeneratorReleaseTarget, Step, StepKind};

use super::{
    apple_linker_check_step, apple_sdk_check_step, binary_format_architecture_check_step,
    generator_release_mise_binary_sha256, gnu_runtime_abi_check_step, help_smoke_check_step,
    mbx_cargo_build_step, native_host_check_step, rust_exec_step, rust_toolchain_check_step,
    setup_rust_steps, version_smoke_check_step,
};
use crate::catalog::{MR_BOXINGTON_VERSION, RUST_VERSION, ToolCatalog};
use crate::steps::ToolHomes;

const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";

fn catalog() -> ToolCatalog {
    ToolCatalog::new(
        "1.97.0", "1.20.0", "2.100.0", "1.7.11", "0.10.0", "1.30.0", "0.9.145", "1.13.0",
    )
    .expect("test catalog versions must be exact")
}

fn strings(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

fn native_check_steps(target: GeneratorReleaseTarget) -> Vec<Step> {
    let catalog = ToolCatalog::pinned();
    let homes = ToolHomes::runner_temp();
    let binary = Path::new("target/release/velnor-actions");
    let mut steps = vec![
        native_host_check_step(target, &homes, &catalog).expect("host check must build"),
        rust_toolchain_check_step(target, "1.98.1", &homes, &catalog)
            .expect("toolchain check must build"),
        binary_format_architecture_check_step(target, binary, &homes, &catalog)
            .expect("binary check must build"),
    ];
    match target {
        GeneratorReleaseTarget::LinuxX86_64 => steps.push(
            gnu_runtime_abi_check_step(target, binary, &homes, &catalog)
                .expect("GNU ABI check must build on Linux"),
        ),
        GeneratorReleaseTarget::MacosArm64 => {
            steps.push(
                apple_sdk_check_step(target, &homes, &catalog).expect("SDK check must build"),
            );
            steps.push(
                apple_linker_check_step(target, &homes, &catalog).expect("linker check must build"),
            );
        }
    }
    steps.push(
        version_smoke_check_step(binary, "0.1.1", &homes, &catalog)
            .expect("version check must build"),
    );
    steps.push(help_smoke_check_step(binary, &homes, &catalog).expect("help check must build"));
    steps
}

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

#[test]
fn native_checks_use_control_free_argv_and_keep_platform_guards() {
    for (target, expected_host, expected_triple, expected_names) in [
        (
            GeneratorReleaseTarget::LinuxX86_64,
            "Linux x86_64",
            "x86_64-unknown-linux-gnu",
            [
                "Verify native build host",
                "Verify selected Rust toolchain",
                "Verify binary format and exact architecture",
                "Verify Ubuntu 22.04 GNU ABI baseline",
                "Smoke test release binary version",
                "Smoke test release binary help",
            ]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        ),
        (
            GeneratorReleaseTarget::MacosArm64,
            "Darwin arm64",
            "aarch64-apple-darwin",
            [
                "Verify native build host",
                "Verify selected Rust toolchain",
                "Verify binary format and exact architecture",
                "Observe selected Apple SDK",
                "Observe selected Apple linker",
                "Smoke test release binary version",
                "Smoke test release binary help",
            ]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        ),
    ] {
        let steps = native_check_steps(target);
        let names = steps
            .iter()
            .map(|step| step.name.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names, expected_names,
            "native check inventory for {target:?}"
        );

        for step in steps {
            let StepKind::Shell { run, .. } = step.kind else {
                panic!("native check must use a shell step: {}", step.name);
            };
            assert!(
                run.iter().all(|argument| {
                    !argument.chars().any(char::is_control) && !argument.contains("$(")
                }),
                "unsafe shell token in {target:?} argv for {}: {run:?}",
                step.name
            );
            let script = run.last().expect("native-check argv must carry its script");
            match step.name.as_str() {
                "Verify native build host" => {
                    assert!(script.contains("uname -sm"));
                    let host_clause = format!("expected='{expected_host}'");
                    assert!(script.contains(host_clause.as_str()));
                }
                "Verify selected Rust toolchain" => {
                    assert!(script.contains("rustc -vV"));
                    assert!(script.contains("'release: 1.98.1'"));
                    let triple_clause = format!("'host: {expected_triple}'");
                    assert!(script.contains(triple_clause.as_str()));
                }
                "Verify binary format and exact architecture"
                    if target == GeneratorReleaseTarget::LinuxX86_64 =>
                {
                    assert!(script.contains("readelf -hW"));
                    assert!(script.contains("ELF64"));
                    assert!(script.contains("Advanced Micro Devices X86-64"));
                }
                "Verify binary format and exact architecture" => {
                    assert!(script.contains("file -b"));
                    assert!(script.contains("lipo -archs"));
                    assert!(script.contains("arm64"));
                }
                "Verify Ubuntu 22.04 GNU ABI baseline" => {
                    assert!(script.contains("readelf --version-info --wide"));
                    assert!(script.contains("GLIBC_"));
                    assert!(script.contains("ldd -v"));
                    assert!(script.contains("'not found'"));
                }
                "Observe selected Apple SDK" => {
                    assert!(script.contains("xcrun --show-sdk-path"));
                    assert!(script.contains("test -d \"$sdk\""));
                }
                "Observe selected Apple linker" => {
                    assert!(script.contains("xcrun --find clang"));
                    assert!(script.contains("xcrun --find ld"));
                    assert!(script.contains("test -x \"$clang\""));
                    assert!(script.contains("test -x \"$linker\""));
                }
                "Smoke test release binary version" => {
                    assert!(script.contains("--version"));
                    assert!(script.contains("'velnor-actions 0.1.1'"));
                }
                "Smoke test release binary help" => {
                    assert!(script.contains("--help"));
                    assert!(script.contains("Usage: velnor-actions <COMMAND>"));
                    for command in ["init", "plan", "generate", "config"] {
                        assert!(script.contains(command));
                    }
                }
                name => panic!("unexpected native-check step: {name}"),
            }
        }
    }
}

#[test]
fn rust_exec_remains_available_for_non_compilation_checks() {
    let catalog = catalog();
    let homes = ToolHomes::runner_temp();
    let args = strings(&["-euo", "pipefail", "-c", "test -x target/release/tool"]);
    let step = rust_exec_step(
        "Check release binary",
        OsStr::new("bash"),
        &args,
        &homes,
        &catalog,
    )
    .expect("non-compilation native checks must keep the Rust exec path");

    let StepKind::Shell { run, .. } = step.kind else {
        panic!("the native check must remain a shell step");
    };
    assert!(run.windows(2).any(|pair| pair == ["exec", "rust@1.97.0"]));
    assert!(run.windows(2).any(|pair| pair == ["--", "bash"]));
    assert!(!run.iter().any(|argument| argument == "mr-boxington@1.20.0"));
    assert!(!run.iter().any(|argument| argument == "mbx"));
}
