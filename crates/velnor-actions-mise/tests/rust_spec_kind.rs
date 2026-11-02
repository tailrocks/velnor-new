// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_mise::{RustSpecKind, rust_spec_kind};

#[test]
fn rust_spec_kind_accepts_only_the_plain_and_minimal_specs() {
    let rows = [
        (
            "rust[profile=minimal]@1.99.1",
            RustSpecKind::Minimal("1.99.1"),
        ),
        ("rust@1.99.1", RustSpecKind::Plain("1.99.1")),
        (
            "rust[profile=complete]@1.99.1",
            RustSpecKind::UnsupportedRust,
        ),
        (
            "rust[profile=minimal,components=rust-docs]@1.99.1",
            RustSpecKind::UnsupportedRust,
        ),
        ("rust[profile=minimal@1.99.1", RustSpecKind::UnsupportedRust),
        ("rust[profile=minimal]", RustSpecKind::UnsupportedRust),
        ("core:rust@1.99.1", RustSpecKind::UnsupportedRust),
        (
            "core:rust[profile=complete]@1.99.1",
            RustSpecKind::UnsupportedRust,
        ),
        (
            "core:rust[profile=minimal]@1.99.1",
            RustSpecKind::UnsupportedRust,
        ),
        ("gh@2.103.2", RustSpecKind::NotRust),
        (
            "aqua:nextest-rs/nextest/cargo-nextest@0.9.150",
            RustSpecKind::NotRust,
        ),
        ("rust-analyzer@1.0.0", RustSpecKind::NotRust),
        ("rustfmt@1.0.0", RustSpecKind::NotRust),
    ];
    for (spec, kind) in rows {
        assert_eq!(rust_spec_kind(spec), kind, "{spec} was misclassified");
    }
}
