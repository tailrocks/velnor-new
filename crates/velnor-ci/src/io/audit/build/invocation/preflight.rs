// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Invocation;

fn words(line: &str) -> Vec<String> {
    line.split(' ').map(str::to_owned).collect()
}

#[test]
fn the_no_run_preflight_drops_the_fail_fast_flags_nextest_refuses_beside_no_run() {
    for flag in ["--no-fail-fast", "--fail-fast"] {
        let command = words(&format!(
            "cargo-nextest nextest run --workspace --locked {flag} --profile ci"
        ));
        let preflight = Invocation::new(&command)
            .preflight
            .unwrap_or_else(|| unreachable!("a nextest run has a preflight"));
        assert!(preflight.contains(&"--no-run".to_owned()), "{preflight:?}");
        assert!(!preflight.contains(&flag.to_owned()), "{preflight:?}");
        assert!(preflight.contains(&"--profile".to_owned()), "{preflight:?}");
    }
}

#[test]
fn the_preflight_keeps_the_arguments_after_the_separator_untouched() {
    let command = words("cargo test --workspace -- --no-fail-fast");
    let preflight = Invocation::new(&command)
        .preflight
        .unwrap_or_else(|| unreachable!("a cargo test has a preflight"));
    assert_eq!(
        preflight,
        words("cargo test --workspace --no-run -- --no-fail-fast")
    );
}

#[test]
fn a_run_is_prepared_by_building_what_it_would_run() {
    for (command, build) in [
        (
            "cargo run -p lib --bin tool --release -- --flag",
            "cargo build -p lib --bin tool --release",
        ),
        ("cargo r -p lib --bin tool", "cargo build -p lib --bin tool"),
        (
            "cargo --locked -C rust run -p lib --example demo",
            "cargo --locked -C rust build -p lib --example demo",
        ),
    ] {
        let invocation = Invocation::new(&words(command));
        assert_eq!(invocation.preflight, Some(words(build)), "{command}");
        assert!(invocation.cargo && !invocation.compiler, "{command}");
    }
}

#[test]
fn a_bench_is_prepared_like_a_test() {
    let invocation = Invocation::new(&words("cargo bench -p lib -- --save-baseline base"));
    assert_eq!(
        invocation.preflight,
        Some(words("cargo bench -p lib --no-run -- --save-baseline base"))
    );
    let compile_only = Invocation::new(&words("cargo bench -p lib --no-run"));
    assert!(compile_only.compiler && compile_only.preflight.is_none());
}

#[test]
fn rustc_rustdoc_and_fix_compile_in_place() {
    for command in [
        "cargo rustc -p lib --lib -- -Zunpretty=expanded",
        "cargo rustdoc -p lib",
    ] {
        let invocation = Invocation::new(&words(command));
        assert!(
            invocation.cargo && invocation.compiler && invocation.preflight.is_none(),
            "{command}"
        );
    }
}

#[test]
fn a_run_preflight_drops_the_positional_arguments_of_the_program() {
    for (command, build) in [
        (
            "cargo run --offline -p lib --bin tool input.txt",
            "cargo build --offline -p lib --bin tool",
        ),
        (
            "cargo run -p lib --bin tool --features extra data other",
            "cargo build -p lib --bin tool --features extra",
        ),
        (
            "cargo run -p=lib --bin=tool --profile dev",
            "cargo build -p=lib --bin=tool --profile dev",
        ),
    ] {
        let invocation = Invocation::new(&words(command));
        assert_eq!(invocation.preflight, Some(words(build)), "{command}");
    }
}

#[test]
fn an_opened_documentation_build_is_stabilized_before_it_is_opened() {
    for (command, compile) in [
        ("cargo rustdoc -p lib --open", "cargo rustdoc -p lib"),
        ("cargo doc --workspace --open", "cargo doc --workspace"),
    ] {
        let invocation = Invocation::new(&words(command));
        assert_eq!(invocation.preflight, Some(words(compile)), "{command}");
        assert!(!invocation.compiler, "{command}");
    }
}

#[test]
fn a_command_whose_compile_cannot_be_trusted_or_named_is_refused() {
    for command in [
        "cargo fix --lib --allow-no-vcs",
        "cargo run --offline",
        "cargo run -p lib",
        "cargo run --bin tool",
        "cargo r input.txt",
    ] {
        assert!(
            Invocation::new(&words(command)).refusal.is_some(),
            "{command}"
        );
    }
    for command in [
        "cargo run -p lib --bin tool",
        "cargo r --package=lib --example=demo -- input.txt",
        "cargo build",
    ] {
        assert!(
            Invocation::new(&words(command)).refusal.is_none(),
            "{command}"
        );
    }
}
