// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use velnor_ci::tools_verify::{Discard, Verdict, digest, read};

use crate::tools_verify::{Job, job};

const COMPONENTS: &str =
    ".rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/lib/rustlib/components";

fn write(job: &Job, relative: &str, content: &str) {
    let path = job.home.join(relative);
    fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(path, content).unwrap_or_else(|e| unreachable!("{e}"));
}

fn digest_of(job: &Job) -> String {
    digest(&job.home).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn the_order_rustup_listed_the_components_in_does_not_change_the_digest() {
    let first = job("tools-digest-order-a");
    let second = job("tools-digest-order-b");
    write(
        &first,
        COMPONENTS,
        "rust-std-x86_64\nrust-std-aarch64\nrustc\n",
    );
    write(
        &second,
        COMPONENTS,
        "rustc\nrust-std-aarch64\nrust-std-x86_64\n",
    );
    assert_eq!(digest_of(&first), digest_of(&second));
}

#[test]
fn a_changed_component_list_still_changes_the_digest() {
    let base = job("tools-digest-set-a");
    write(&base, COMPONENTS, "rust-std-x86_64\nrustc\n");
    let reference = digest_of(&base);
    for changed in [
        "rust-std-x86_64\nrustc\nclippy-preview\n",
        "rust-std-x86_64\n",
        "rust-std-x86_64\nrustc\nrustc\n",
        "rust-std-x86_64\nrustc",
        "rust-std-x86_64 \nrustc\n",
    ] {
        let other = job("tools-digest-set-b");
        write(&other, COMPONENTS, changed);
        assert_ne!(reference, digest_of(&other), "{changed:?}");
    }
}

#[test]
fn only_the_component_list_is_read_as_a_set() {
    let first = job("tools-digest-other-a");
    let second = job("tools-digest-other-b");
    for (job, content) in [(&first, "a\nb\n"), (&second, "b\na\n")] {
        write(
            job,
            ".rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/lib/rustlib/other",
            content,
        );
        write(
            job,
            ".rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/components",
            content,
        );
    }
    assert_ne!(digest_of(&first), digest_of(&second));
}

#[test]
fn an_anchor_of_the_unordered_digest_is_not_trusted() {
    let job = job("tools-digest-old-anchor");
    let digest = digest_of(&job);
    fs::create_dir_all(job.anchor.parent().unwrap_or(Path::new(".")))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&job.anchor, format!("velnor-tools-anchor-v1 {digest}\n"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        read(&job.anchor, true),
        Verdict::Unanchored(Discard::BadAnchor)
    );
    fs::write(&job.anchor, format!("velnor-tools-anchor-v2 {digest}\n"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(read(&job.anchor, true), Verdict::Anchored(digest));
}
