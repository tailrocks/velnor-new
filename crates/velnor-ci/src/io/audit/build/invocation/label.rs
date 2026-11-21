// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Invocation;

fn label(line: &str) -> String {
    let command: Vec<String> = line.split(' ').map(str::to_owned).collect();
    Invocation::new(&command).label()
}

#[test]
fn a_global_option_before_the_subcommand_does_not_hide_the_subcommand() {
    for line in [
        "cargo --manifest-path /work/app/Cargo.toml build -p lib",
        "cargo --manifest-path=/work/app/Cargo.toml build -p lib",
        "cargo -C /work/app build -p lib",
        "cargo -C/work/app build -p lib",
        "cargo -j4 build -p lib",
        "cargo -j 4 build -p lib",
        "cargo --jobs 4 build -p lib",
        "cargo --jobs=4 build -p lib",
    ] {
        assert_eq!(label(line), "cargo build", "{line}");
    }
}

#[test]
fn a_global_option_value_is_never_part_of_the_label() {
    for line in [
        "cargo --manifest-path /secret-dir/Cargo.toml build",
        "cargo -C /secret-dir build",
        "cargo -j secret-dir test",
    ] {
        let recorded = label(line);
        assert!(!recorded.contains("secret-dir"), "{recorded}");
    }
}
