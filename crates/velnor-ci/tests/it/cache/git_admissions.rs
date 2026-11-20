// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::public_git::{SHA, decide_with, git, public, refused, serve};
use crate::io::Repo as Checkout;

fn empty_home(name: &str) -> (Checkout, String) {
    let repo = Checkout::empty(name);
    let home = repo.root().join("home");
    fs::create_dir_all(&home).unwrap_or_else(|e| unreachable!("{e}"));
    let home = home.to_string_lossy().into_owned();
    (repo, home)
}

#[test]
fn a_private_repository_admits_a_private_source_beside_a_declared_public_one() {
    let mock = serve(public("acme/shared-lib"));
    let (_repo, home) = empty_home("git-admission-mixed");
    let sources = [
        ("dep", git("acme/shared-lib", SHA)),
        (
            "secret",
            "git+ssh://git@github.com/acme/private#abc".to_owned(),
        ),
    ];
    let (output, written) = decide_with(
        &sources,
        &mock.url,
        &[("VELNOR_REPOSITORY_PRIVATE", "true"), ("HOME", &home)],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n", "{output:?}");
    assert_eq!(mock.requests().len(), 0, "{:?}", mock.requests());
}

#[test]
fn an_unpinned_git_source_needs_a_confirmed_private_repository() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [(
        "dep",
        "git+https://github.com/acme/shared-lib?branch=main".to_owned(),
    )];
    for verdict in ["false", "yes", ""] {
        let (output, written) = decide_with(
            &sources,
            &mock.url,
            &[("VELNOR_REPOSITORY_PRIVATE", verdict)],
        );
        refused(&output, &written);
    }
    let (_repo, home) = empty_home("git-admission-unpinned");
    let (output, written) = decide_with(
        &sources,
        &mock.url,
        &[("VELNOR_REPOSITORY_PRIVATE", "true"), ("HOME", &home)],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n", "{output:?}");
    assert_eq!(mock.requests().len(), 0, "{:?}", mock.requests());
}
