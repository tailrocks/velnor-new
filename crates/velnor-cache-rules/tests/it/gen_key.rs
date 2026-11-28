// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{GenKey, GenerationError, Sha};

use crate::gen_support::{key, sha};

#[test]
fn generation_key_names_job_root_and_sixteen_compat_digits() {
    let named = key("rust", "root", "0123456789abcdef0123");
    assert_eq!(named.dir_name(), "rust.root-0123456789abcdef");
}

#[test]
fn generation_keys_differ_by_job_root_and_compat() {
    let base = key("rust", "root", "0123456789abcdef");
    for other in [
        key("lint", "root", "0123456789abcdef"),
        key("rust", "web", "0123456789abcdef"),
        key("rust", "root", "fedcba9876543210"),
    ] {
        assert_ne!(base, other);
        assert_ne!(base.dir_name(), other.dir_name());
    }
}

#[test]
fn generation_key_rejects_names_that_would_leave_their_directory() {
    for (job, root, compat) in [
        ("", "root", "0123456789abcdef"),
        ("rust", "", "0123456789abcdef"),
        ("ru.st", "root", "0123456789abcdef"),
        ("rust", "ro/ot", "0123456789abcdef"),
        ("rust", "root", "0123456789abcde"),
        ("rust", "root", "0123456789abcdeg"),
        ("rust", "root", "0123456789ABCDEF"),
    ] {
        assert!(
            GenKey::new(job, root, compat).is_err(),
            "{job:?} {root:?} {compat:?}"
        );
    }
}

#[test]
fn generation_key_error_names_the_offending_text() {
    let error = GenKey::new("a.b", "root", "0123456789abcdef").err();
    assert_eq!(error, Some(GenerationError::BadSlug("a.b".to_owned())));
}

#[test]
fn commit_sha_must_be_forty_lowercase_hex_digits() {
    assert_eq!(sha(0xab).as_str(), "ab".repeat(20));
    for text in [
        "",
        &"a".repeat(39),
        &"a".repeat(41),
        &"A".repeat(40),
        &"g".repeat(40),
    ] {
        assert!(Sha::parse(text).is_err(), "{text:?}");
    }
}
