// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Depth;

fn depth(raw: &str) -> Depth {
    Depth::parse(raw).unwrap_or_else(|error| panic!("{raw:?}: {error}"))
}

#[test]
fn an_absent_or_blank_depth_is_the_last_commit() {
    for raw in ["", "  ", "\t\n"] {
        assert_eq!(depth(raw), Depth::Last(1), "{raw:?}");
    }
}

#[test]
fn a_number_is_floored_like_math_floor_of_number() {
    for (raw, count) in [
        ("1", 1),
        ("2.9", 2),
        ("2.", 2),
        (".5e1", 5),
        ("1e2", 100),
        ("1E1", 10),
        ("+3", 3),
        (" 7 ", 7),
        ("0x2", 2),
        ("0X1f", 31),
        ("0o17", 15),
        ("0b101", 5),
        ("1e+1", 10),
        ("9.99e0", 9),
    ] {
        assert_eq!(depth(raw), Depth::Last(count), "{raw:?}");
    }
}

#[test]
fn zero_negative_and_not_a_number_mean_the_whole_history() {
    for raw in [
        "0",
        "0.5",
        "-0",
        "-1",
        "-0.5",
        "-Infinity",
        "deep",
        "1_0",
        "0x",
        "-0x2",
        "0b2",
        "0o8",
        "1e",
        "e1",
        ".",
        "+",
        "1 2",
        "0x1.8",
        "Inf",
        "NaN",
        "inf",
        "infinity",
        "\u{85}1",
    ] {
        assert_eq!(depth(raw), Depth::Full, "{raw:?}");
    }
}

#[test]
fn a_depth_beyond_what_git_can_fetch_is_refused() {
    for raw in ["Infinity", "+Infinity"] {
        let message = Depth::parse(raw).expect_err(raw);
        assert!(message.contains("must be a finite number"), "{message}");
    }
}

#[test]
fn a_huge_finite_depth_saturates() {
    assert_eq!(depth("1e30"), Depth::Last(u64::MAX));
}
