// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::image_deploy::cpu::refuse_cpu_flags;

fn environment(name: &str, value: &str) -> Vec<(String, String)> {
    vec![
        ("PATH".to_owned(), "/usr/bin".to_owned()),
        (name.to_owned(), value.to_owned()),
    ]
}

#[test]
fn cpu_specific_rustflags_in_any_cargo_source_are_refused_in_both_spellings() {
    for name in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
    ] {
        for value in [
            "-C target-cpu=native",
            "-Ctarget_cpu=native",
            "--cfg x\u{1f}-Ctarget-feature=+avx2",
            "-C target_feature=+avx2",
        ] {
            assert!(
                refuse_cpu_flags(&environment(name, value)).is_err(),
                "{name} {value}"
            );
        }
    }
}

#[test]
fn other_flags_and_other_variables_pass() {
    assert!(refuse_cpu_flags(&environment("RUSTFLAGS", "--cfg tokio_unstable")).is_ok());
    assert!(refuse_cpu_flags(&environment("OTHER", "-C target-cpu=native")).is_ok());
    assert!(refuse_cpu_flags(&[]).is_ok());
}
