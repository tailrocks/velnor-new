// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::build_fixture::{DOCKER, Run, Source, executable};

const MISMATCH: &str = "Docker client binary is not the same as the value in .velnor/runner.lock.";

fn refused(source: &Source, run: &Run, message: &str) {
    assert!(!run.ok, "the build was accepted");
    assert!(run.stderr.contains(message), "{}", run.stderr);
    assert_eq!(
        source.text("planted-docker.ran"),
        "",
        "a planted docker ran"
    );
    let names: Vec<String> = fs::read_dir(source.path("out"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.is_empty(), "{names:?}");
}

fn other_docker() -> Vec<u8> {
    format!("{DOCKER}# another build\n").into_bytes()
}

#[test]
fn a_docker_that_build_test_sh_puts_on_path_is_never_run() {
    let source = Source::new("planted-path-docker");
    source.script(concat!(
        "target=\"$(command -v docker)\"\n",
        "cp \"$target\" \"$target.genuine\"\n",
        "printf '#!/bin/sh\\necho planted >> \"$FAKE_DIR/planted-docker.ran\"\\nexec \"$0.genuine\" \"$@\"\\n' > \"$target\""
    ));
    let run = source.run(&[], &[]);
    assert!(run.ok, "{}", run.stderr);
    assert!(
        fs::read_to_string(source.path("bin/docker"))
            .unwrap_or_default()
            .contains("planted"),
        "build-test.sh did not replace docker on PATH"
    );
    assert_eq!(source.text("planted-docker.ran"), "");
    assert!(source.text("docker.log").contains("buildx build"));
    assert!(source.path("out/candidate.json").is_file());
}

#[test]
fn a_staged_docker_that_build_test_sh_rewrites_is_never_run() {
    let source = Source::new("planted-staged-docker");
    source.script(concat!(
        "for copy in ../out/.velnor-job-image-candidate-*/docker; do\n",
        "  [ -f \"$copy\" ] || continue\n",
        "  printf '#!/bin/sh\\necho planted >> \"$FAKE_DIR/planted-docker.ran\"\\n' > \"$copy\"\n",
        "  echo \"$copy\" >> ../tampered\n",
        "done"
    ));
    let run = source.run(&[], &[]);
    assert_ne!(source.text("tampered"), "", "no staged docker was found");
    refused(&source, &run, MISMATCH);
}

#[test]
fn a_docker_client_other_than_the_locked_one_is_refused_before_build_test_sh_runs() {
    let source = Source::new("unlocked-docker");
    source.script("echo ran > \"$PWD/../script.ran\"");
    executable(&source.path("bin/docker"), &other_docker());
    let run = source.run(&[], &[]);
    refused(&source, &run, MISMATCH);
    assert_eq!(source.text("script.ran"), "", "build-test.sh ran");
    assert_eq!(source.text("docker.log"), "", "docker ran");
}

#[test]
fn a_docker_client_that_resolves_into_the_source_tree_is_refused() {
    let source = Source::new("docker-in-root");
    executable(&source.path("checkout/tools/docker"), DOCKER.as_bytes());
    source.commit_all("tools");
    let link = source.path("linked-docker");
    std::os::unix::fs::symlink(source.path("checkout/tools/docker"), &link).unwrap_or_default();
    let run = source.run(&[("--docker", link)], &[]);
    refused(
        &source,
        &run,
        "Docker client must be outside the source tree.",
    );
    assert_eq!(source.text("docker.log"), "", "docker ran");
}

#[test]
fn an_explicit_docker_file_replaces_path_discovery() {
    let source = Source::new("explicit-docker");
    let pinned = source.path("pinned/docker");
    executable(&pinned, DOCKER.as_bytes());
    executable(&source.path("bin/docker"), &other_docker());
    let run = source.run(&[("--docker", pinned)], &[]);
    assert!(run.ok, "{}", run.stderr);
    assert!(source.text("docker.log").contains("buildx build"));
    assert!(source.path("out/candidate.json").is_file());
}
