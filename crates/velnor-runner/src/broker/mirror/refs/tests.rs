// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::is_ref;

#[test]
fn an_object_id_of_either_width_is_a_ref() {
    let sha1 = "ab".repeat(20);
    let sha256 = "cd".repeat(32);
    for body in [
        sha1.clone(),
        format!("{sha1}\n"),
        sha256.clone(),
        format!("{sha256}\n"),
        sha1.to_uppercase(),
    ] {
        assert!(is_ref(body.as_bytes()), "{body:?}");
    }
}

#[test]
fn a_symbolic_ref_names_something_under_refs() {
    assert!(is_ref(b"ref: refs/heads/main\n"));
    assert!(is_ref(b"ref: refs/heads/main"));
}

#[test]
fn everything_else_is_not_a_ref() {
    let long = "a".repeat(41);
    let short = "a".repeat(39);
    let odd = format!("{}g", "a".repeat(39));
    let padded = format!("{} \n", "a".repeat(40));
    for body in [
        b"".as_slice(),
        b"\n",
        b"ref: refs/",
        b"ref: HEAD\n",
        b"ref: refs/a\nb\n",
        b"not a ref\n",
        long.as_bytes(),
        short.as_bytes(),
        odd.as_bytes(),
        padded.as_bytes(),
    ] {
        assert!(!is_ref(body), "{body:?}");
    }
}
