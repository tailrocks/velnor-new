// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::platform::Platform;

fn linux() -> Platform {
    Platform::parse(
        "x86_64-unknown-linux-gnu",
        "debug_assertions\nunix\ntarget_arch=\"x86_64\"\ntarget_os=\"linux\"\ntarget_pointer_width=\"64\"\n",
    )
}

#[test]
fn a_cfg_predicate_holds_when_rustc_prints_the_name_or_the_pair() {
    let platform = linux();
    for (predicate, expected) in [
        ("cfg(unix)", true),
        ("cfg(windows)", false),
        ("cfg(target_os = \"linux\")", true),
        ("cfg(target_os = \"macos\")", false),
        ("cfg(not(windows))", true),
        ("cfg(all(unix, target_pointer_width = \"32\"))", false),
        ("cfg(any(windows, target_arch = \"x86_64\"))", true),
        ("cfg(all())", true),
        ("cfg(any())", false),
    ] {
        assert_eq!(platform.holds(predicate), expected, "{predicate}");
    }
}

#[test]
fn a_plain_platform_holds_only_for_the_host_triple() {
    let platform = linux();
    assert!(platform.holds("x86_64-unknown-linux-gnu"));
    assert!(!platform.holds("aarch64-apple-darwin"));
}

#[test]
fn a_predicate_that_cannot_be_read_is_taken_to_hold() {
    let platform = linux();
    for predicate in [
        "cfg(unix",
        "cfg(nope(unix))",
        "cfg(unix windows)",
        "cfg(not(unix, windows))",
    ] {
        assert!(platform.holds(predicate), "{predicate}");
    }
}

#[test]
fn a_predicate_on_a_cfg_key_that_rustflags_can_change_is_taken_to_hold() {
    let platform = linux();
    for predicate in [
        "cfg(target_feature = \"avx2\")",
        "cfg(not(target_feature = \"crt-static\"))",
        "cfg(all(unix, target_feature = \"sse4.2\"))",
        "cfg(panic = \"abort\")",
        "cfg(debug_assertions)",
    ] {
        assert!(platform.holds(predicate), "{predicate}");
    }
}
