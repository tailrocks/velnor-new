use std::collections::BTreeSet;
use std::ffi::OsStr;
#[cfg(unix)]
use std::io::{self, Write};
use std::path::Path;
#[cfg(unix)]
use std::process::{Command, Stdio};

use velnor_actions_contract::{GeneratorReleaseTarget, Step, StepKind};

#[cfg(unix)]
use super::checks::NATIVE_HOST_GUARD_AWK;
use super::{
    apple_linker_check_step, apple_sdk_check_step, binary_format_architecture_check_step,
    gnu_runtime_abi_check_step, help_smoke_check_step, native_host_check_step, rust_exec_step,
    rust_toolchain_check_step, version_smoke_check_step,
};
use crate::catalog::ToolCatalog;
use crate::steps::ToolHomes;

#[path = "generator_release_workflow_mbx_tests.rs"]
mod mbx_tests;

fn catalog() -> ToolCatalog {
    ToolCatalog::new(
        "1.97.0", "1.20.0", "2.100.0", "1.7.11", "0.10.0", "1.30.0", "0.9.145", "1.13.0",
    )
    .expect("test catalog versions must be exact")
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
        GeneratorReleaseTarget::MacosArm64 | GeneratorReleaseTarget::MacosX86_64 => {
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
        (
            GeneratorReleaseTarget::MacosX86_64,
            "Darwin x86_64",
            "x86_64-apple-darwin",
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
                    assert!(
                        script.contains(if target == GeneratorReleaseTarget::MacosArm64 {
                            "arm64"
                        } else {
                            "x86_64"
                        })
                    );
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

#[cfg(unix)]
fn native_host_guard_accepts(input: &str, expected: &str) -> io::Result<bool> {
    let mut child = Command::new("awk")
        .arg("-v")
        .arg(format!("expected={expected}"))
        .arg(NATIVE_HOST_GUARD_AWK)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }
    Ok(child.wait()?.success())
}

#[cfg(unix)]
#[test]
fn native_host_guard_requires_exactly_one_expected_host_line() -> io::Result<()> {
    let expected = "Linux x86_64";

    assert!(native_host_guard_accepts("Linux x86_64\n", expected)?);
    assert!(!native_host_guard_accepts("", expected)?);
    assert!(!native_host_guard_accepts(
        "Linux x86_64\nextra\n",
        expected
    )?);
    assert!(!native_host_guard_accepts("Linux aarch64\n", expected)?);

    Ok(())
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
