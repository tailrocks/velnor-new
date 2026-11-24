// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::build_fixture::{Run, Source};

fn refused_before_anything_ran(source: &Source, run: &Run) {
    assert!(!run.ok, "a secret in the helper environment was accepted");
    assert_eq!(source.text("script.ran"), "", "build-test.sh ran");
    assert_eq!(source.text("docker.log"), "", "docker ran");
    let written: Vec<_> = fs::read_dir(source.path("out"))
        .into_iter()
        .flatten()
        .collect();
    assert!(written.is_empty(), "{written:?}");
}

#[test]
fn a_token_in_the_helper_environment_is_refused_before_build_test_sh_runs() {
    let source = Source::new("secret-token");
    source.script("echo ran > \"$PWD/../script.ran\"");
    let run = source.run(&[], &[("GH_TOKEN", Some("secret-token"))]);
    refused_before_anything_ran(&source, &run);
    assert!(run.stderr.contains("GH_TOKEN"), "{}", run.stderr);
    assert!(!run.stderr.contains("secret-token"), "{}", run.stderr);
}

#[test]
fn every_secret_marker_in_any_case_is_refused_and_only_git_author_names_are_exempt() {
    let source = Source::new("secret-names");
    source.script("echo ran > \"$PWD/../script.ran\"");
    let run = source.run(
        &[],
        &[
            ("ACTIONS_ID_TOKEN_REQUEST_TOKEN", Some("x")),
            ("registry_password", Some("x")),
            ("AWS_SECRET_ACCESS_KEY", Some("x")),
            ("GOOGLE_APPLICATION_CREDENTIALS", Some("x")),
            ("DOCKER_AUTH_CONFIG", Some("x")),
            ("DEPLOYKEY", Some("")),
            ("GIT_AUTHOR_NAME", Some("x")),
            ("GIT_AUTHOR_EMAIL", Some("x")),
            ("GIT_AUTHOR_DATE", Some("x")),
        ],
    );
    refused_before_anything_ran(&source, &run);
    assert!(
        run.stderr.contains(
            ": ACTIONS_ID_TOKEN_REQUEST_TOKEN, AWS_SECRET_ACCESS_KEY, DEPLOYKEY, \
             DOCKER_AUTH_CONFIG, GOOGLE_APPLICATION_CREDENTIALS, registry_password."
        ),
        "{}",
        run.stderr
    );
}

#[test]
fn git_author_variables_alone_do_not_stop_the_build() {
    let source = Source::new("git-author");
    let run = source.run(
        &[],
        &[
            ("GIT_AUTHOR_NAME", Some("t")),
            ("GIT_AUTHOR_EMAIL", Some("t@example.com")),
            ("GIT_AUTHOR_DATE", Some("2026-01-02T03:04:05Z")),
        ],
    );
    assert!(run.ok, "{}", run.stderr);
    assert!(source.path("out/candidate.json").is_file());
}
