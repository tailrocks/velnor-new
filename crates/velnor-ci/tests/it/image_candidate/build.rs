// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::PathBuf;

use super::build_fixture::{Run, Source};

const LEGACY_RECORD: &str = include_str!("../data/image_build_record.json");

fn left_behind(source: &Source) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(source.path("out"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn writes_the_record_the_legacy_builder_writes_and_verify_candidate_accepts_it() {
    let source = Source::new("legacy-bytes");
    let run = source.run(&[], &[]);
    assert!(run.ok, "{}", run.stderr);
    assert!(run.stdout.starts_with("PASS"), "{}", run.stdout);
    let expected = LEGACY_RECORD.replace("{COMMIT}", &source.head());
    assert_eq!(source.text("out/candidate.json"), expected);
    assert_eq!(
        fs::read(source.path("out/candidate.oci.tar")).unwrap_or_default(),
        fs::read(source.path("archive.tar")).unwrap_or_default()
    );
    assert_eq!(
        left_behind(&source),
        ["candidate.json", "candidate.oci.tar"]
    );
    let verified = source.verify();
    assert!(verified.ok, "{}", verified.stderr);
}

#[test]
fn builds_into_a_local_archive_without_a_registry_or_credentials() {
    let source = Source::new("local-only");
    let run = source.run(&[], &[]);
    assert!(run.ok, "{}", run.stderr);
    let log = source.text("docker.log");
    let calls: Vec<&str> = log.lines().collect();
    assert_eq!(calls.len(), 3, "{log}");
    assert_eq!(calls[0], "info --format {{json .ClientInfo.Plugins}}");
    assert_eq!(calls[1], "buildx version");
    assert!(
        calls[2].starts_with("buildx build --platform linux/amd64 --file "),
        "{}",
        calls[2]
    );
    assert!(calls[2].contains(" --provenance=false --output type=oci,dest="));
    assert!(calls[2].contains("/candidate.oci.tar,compression=gzip,force-compression=true "));
    assert!(!log.contains("push") && !log.contains("login"));
    let env = source.text("docker.env");
    for hidden in ["GH_TOKEN", "REGISTRY_PASSWORD", "DOCKER_HOST"] {
        assert!(!env.contains(hidden), "{hidden} reached docker build");
    }
    assert!(env.contains("/docker-config\n"), "{env}");
}

fn refused(label: &str, act: impl Fn(&Source) -> Run, message: &str) {
    let source = Source::new(label);
    let run = act(&source);
    assert!(!run.ok, "{label} was accepted");
    assert!(run.stderr.contains(message), "{label}: {}", run.stderr);
    let names = left_behind(&source);
    assert!(
        names
            .iter()
            .all(|name| name != "candidate.oci.tar" && !name.starts_with('.')),
        "{label} left {names:?}"
    );
}

macro_rules! refusal {
    ($name:ident, $message:expr, $act:expr) => {
        #[test]
        fn $name() {
            refused(stringify!($name), $act, $message);
        }
    };
}

refusal!(
    a_worktree_with_an_untracked_file_is_refused,
    "Build only when the Git worktree is clean.",
    |source| {
        fs::write(source.path("checkout/stray"), "x").unwrap_or_default();
        source.run(&[], &[])
    }
);

refusal!(
    a_source_commit_other_than_the_worktree_head_is_refused,
    "Requested source commit is not the same as the Git commit in the worktree.",
    |source| source.run(&[("--source-commit", PathBuf::from("b".repeat(40)))], &[])
);

refusal!(
    a_github_sha_other_than_the_worktree_head_is_refused,
    "GitHub source commit is not the same as the Git commit in the worktree.",
    |source| source.run(&[], &[("GITHUB_SHA", Some(&"b".repeat(40)))])
);

refusal!(
    a_buildx_binary_other_than_the_locked_one_is_refused,
    "Docker Buildx binary is not the same as the value in .velnor/runner.lock.",
    |source| {
        fs::write(source.path("buildx"), "tampered").unwrap_or_default();
        source.run(&[], &[])
    }
);

#[test]
fn a_tampered_buildx_binary_is_never_run_by_the_buildx_probe() {
    let source = Source::new("tampered-plugin");
    fs::write(source.path("buildx"), "tampered").unwrap_or_default();
    assert!(!source.run(&[], &[]).ok);
    let log = source.text("docker.log");
    assert_eq!(log.trim(), "info --format {{json .ClientInfo.Plugins}}");
}

refusal!(
    a_recorded_input_that_is_a_symlink_is_refused,
    "images/job/ubuntu-26.04/README.md must be a regular file.",
    |source| {
        let readme = source.path("checkout/images/job/ubuntu-26.04/README.md");
        fs::remove_file(&readme).unwrap_or_default();
        std::os::unix::fs::symlink("/dev/zero", &readme).unwrap_or_default();
        source.commit_all("link");
        source.run(&[], &[])
    }
);

refusal!(
    an_archive_inside_the_source_tree_is_refused,
    "Candidate outputs must stay outside the source tree.",
    |source| source.run(
        &[("--archive", source.path("checkout/candidate.oci.tar"))],
        &[]
    )
);

refusal!(
    outputs_in_two_directories_are_refused,
    "Candidate outputs must use separate files in one directory.",
    |source| {
        fs::create_dir(source.path("elsewhere")).unwrap_or_default();
        source.run(
            &[("--record", source.path("elsewhere/candidate.json"))],
            &[],
        )
    }
);

refusal!(
    an_existing_record_is_refused,
    "Candidate outputs must be new, separate files.",
    |source| {
        fs::write(source.path("out/candidate.json"), "old").unwrap_or_default();
        source.run(&[], &[])
    }
);

refusal!(
    a_symlinked_record_path_is_refused,
    "Candidate output paths cannot use symlinks.",
    |source| {
        std::os::unix::fs::symlink(source.path("nowhere"), source.path("out/candidate.json"))
            .unwrap_or_default();
        source.run(&[], &[])
    }
);

refusal!(
    a_failing_docker_build_leaves_no_output,
    "failed",
    |source| {
        source.docker("exit 3");
        source.run(&[], &[])
    }
);

refusal!(
    a_corrupt_archive_from_docker_leaves_no_output,
    "OCI archive",
    |source| {
        source.docker("echo corrupt > \"$dest\"\n  exit 0");
        source.run(&[], &[])
    }
);

refusal!(
    a_validation_script_that_edits_a_recorded_input_is_refused,
    "Build only when the Git worktree is clean.",
    |source| {
        source.script("echo changed >> images/job/ubuntu-26.04/Dockerfile");
        source.run(&[], &[])
    }
);

#[test]
fn the_validation_script_gets_no_credentials_and_leaves_no_process_behind() {
    let source = Source::new("script-env");
    source.script("sleep 30 &\nenv > \"$PWD/../script.env\"");
    let run = source.run(&[], &[]);
    assert!(run.ok, "{}", run.stderr);
    let seen = source.text("script.env");
    assert!(seen.contains("PATH="), "{seen}");
    for hidden in ["GH_TOKEN", "REGISTRY_PASSWORD", "DOCKER_HOST"] {
        assert!(!seen.contains(hidden), "{hidden} reached build-test.sh");
    }
}
