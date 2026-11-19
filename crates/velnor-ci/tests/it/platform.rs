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
        ("cfg(true)", true),
        ("cfg(false)", false),
        ("cfg(not(true))", false),
        ("cfg(not(false))", true),
        ("cfg(all(unix, true))", true),
        ("cfg(any(false, windows))", false),
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
fn a_predicate_on_a_name_or_key_outside_rustcs_target_cfgs_is_taken_to_hold() {
    let platform = linux();
    for predicate in [
        "cfg(tokio_unstable)",
        "cfg(debug_assertions)",
        "cfg(not(debug_assertions))",
        "cfg(target_feature = \"x\")",
        "cfg(not(target_feature = \"crt-static\"))",
        "cfg(all(unix, my_flag))",
        "cfg(all(unix, panic = \"abort\"))",
        "cfg(overflow_checks)",
        "cfg(relocation_model = \"pic\")",
        "cfg(sanitize = \"address\")",
        "cfg(feature = \"x\")",
        "cfg(any(windows, tokio_unstable))",
        "cfg(unix = \"x\")",
        "cfg(windows = \"x\")",
        "cfg(not(unix = \"x\"))",
        "cfg(target_os)",
        "cfg(target_has_atomic)",
    ] {
        assert!(platform.holds(predicate), "{predicate}");
    }
}

#[test]
fn a_predicate_that_only_names_target_cfgs_can_be_inactive() {
    let platform = linux();
    for predicate in [
        "cfg(windows)",
        "cfg(all(windows, tokio_unstable))",
        "cfg(all(windows, target_feature = \"avx2\"))",
        "cfg(all(target_feature = \"avx2\", windows))",
        "cfg(all(unix, target_os = \"macos\"))",
        "cfg(target_has_atomic = \"512\")",
        "cfg(target_env = \"msvc\")",
        "cfg(not(unix))",
    ] {
        assert!(!platform.holds(predicate), "{predicate}");
    }
}

#[test]
fn an_unknown_target_keeps_every_predicate_that_names_a_cfg_and_decides_the_literal_ones() {
    let platform = Platform::unknown();
    for (predicate, expected) in [
        ("cfg(unix)", true),
        ("cfg(target_os = \"linux\")", true),
        ("cfg(any(false, unix))", true),
        ("cfg(false)", false),
        ("cfg(not(true))", false),
        ("cfg(all(false, custom_flag))", false),
        ("cfg(true)", true),
        ("x86_64-unknown-linux-gnu", true),
    ] {
        assert_eq!(platform.holds(predicate), expected, "{predicate}");
    }
}
