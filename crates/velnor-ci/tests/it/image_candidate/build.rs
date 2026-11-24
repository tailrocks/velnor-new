// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use super::build_fixture::{PLANTED, Run, Source};
use super::fixture::{LAYER, build, plain_layer, spec};

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
    assert_eq!(calls.len(), 2, "{log}");
    assert_eq!(calls[0], "buildx version");
    assert!(
        calls[1].starts_with("buildx build --platform linux/amd64 --file "),
        "{}",
        calls[1]
    );
    let argument = format!(
        "/Dockerfile --build-arg VELNOR_SOURCE_SHA={} --provenance=false --output type=oci,dest=",
        source.head()
    );
    assert!(calls[1].contains(&argument), "{}", calls[1]);
    assert!(calls[1].contains("/candidate.oci.tar,compression=gzip,force-compression=true "));
    assert!(!log.contains("push") && !log.contains("login"));
    let env = source.text("docker.env");
    for hidden in ["GH_TOKEN", "REGISTRY_PASSWORD", "DOCKER_HOST"] {
        assert!(!env.contains(hidden), "{hidden} reached docker build");
    }
    assert!(env.contains("/docker-config\n"), "{env}");
    assert!(
        env.contains("HOME=") && env.contains("/.velnor-job-image-candidate-"),
        "docker build kept the real home directory: {env}"
    );
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
        source.plugin(b"#!/bin/sh\necho tampered\n");
        source.run(&[], &[])
    }
);

#[test]
fn a_plugin_that_swaps_itself_for_the_genuine_bytes_is_never_run() {
    let source = Source::new("planted-plugin");
    source.plugin(PLANTED);
    let run = source.run(&[], &[]);
    assert!(!run.ok, "a planted plugin was accepted");
    assert!(
        run.stderr.contains("Docker Buildx binary is not the same"),
        "{}",
        run.stderr
    );
    assert_eq!(source.text("planted.ran"), "");
    assert_eq!(source.text("plugin.ran"), "");
    assert_eq!(source.text("docker.log"), "");
}

#[test]
fn an_explicit_buildx_path_replaces_discovery() {
    let source = Source::new("explicit-plugin");
    let elsewhere = source.path("elsewhere/docker-buildx");
    fs::create_dir_all(source.path("elsewhere")).unwrap_or_default();
    fs::copy(source.path("genuine-plugin"), &elsewhere).unwrap_or_default();
    source.plugin(PLANTED);
    let run = source.run(&[("--buildx", elsewhere)], &[]);
    assert!(run.ok, "{}", run.stderr);
    assert_eq!(source.text("planted.ran"), "");
    assert!(source.path("out/candidate.json").is_file());
}

#[test]
fn a_termination_signal_stops_the_docker_build_and_removes_the_staging_directory() {
    let source = Source::new("signal");
    source.docker(
        "echo $$ > \"$FAKE_DIR/build.pid\"\nsleep 60 &\necho $! > \"$FAKE_DIR/sleep.pid\"\nwait",
    );
    let mut child = source.spawn();
    let pids = ["build.pid", "sleep.pid"];
    let started = Instant::now();
    while pids.iter().any(|name| source.text(name).is_empty()) {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "build never started"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let velnor = Pid::from_raw(i32::try_from(child.id()).unwrap_or(0));
    kill(velnor, Signal::SIGTERM).unwrap_or_default();
    let status = child.wait().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!status.success());
    for name in pids {
        let pid = source.text(name).trim().parse::<i32>().unwrap_or(0);
        let gone = (0..200).any(|_| {
            std::thread::sleep(Duration::from_millis(5));
            kill(Pid::from_raw(pid), None).is_err()
        });
        assert!(gone, "{name} is still running");
    }
    assert_eq!(left_behind(&source), Vec::<String>::new());
}

refusal!(
    a_root_below_the_worktree_top_is_refused,
    "--root must be the top directory of the Git worktree.",
    |source| source.run(&[("--root", source.path("checkout/images"))], &[])
);

refusal!(
    a_buildx_file_over_the_size_limit_is_refused_before_it_is_copied,
    "Docker Buildx plugin exceeds its size limit.",
    |source| {
        let huge = source.path("huge-buildx");
        let file = fs::File::create(&huge).unwrap_or_else(|e| unreachable!("{e}"));
        file.set_len((1 << 30) + 1).unwrap_or_default();
        source.run(&[("--buildx", huge)], &[])
    }
);

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
    an_image_that_does_not_carry_the_source_commit_leaves_no_output,
    "OCI image configuration revision label is not the source commit.",
    |source| {
        let mut candidate = spec(vec![(plain_layer(), LAYER)]);
        candidate.revision = Some("b".repeat(40));
        fs::write(source.path("unlabeled.tar"), build(&candidate).archive()).unwrap_or_default();
        source.docker("cp \"$FAKE_DIR/unlabeled.tar\" \"$dest\"\nexit 0");
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
fn a_failing_build_reports_its_last_line_after_a_megabyte_of_output() {
    let source = Source::new("long-failure");
    source.docker(concat!(
        "i=0\n",
        "while [ $i -lt 20000 ]; do\n",
        "  echo \"progress progress progress progress progress progress progress progress\" >&2\n",
        "  i=$((i+1))\n",
        "done\n",
        "echo \"the-real-error-line\" >&2\n",
        "exit 3"
    ));
    let run = source.run(&[], &[]);
    assert!(!run.ok);
    assert!(run.stderr.contains("the-real-error-line"), "lost the error");
    assert!(run.stderr.contains("failed"), "{}", run.stderr);
}

#[test]
fn the_progress_of_a_successful_build_is_streamed() {
    let source = Source::new("streamed-progress");
    source.docker("echo \"build-progress-line\" >&2\necho \"build-stdout-line\"\ncp \"$FAKE_DIR/archive.tar\" \"$dest\"\nexit 0");
    let run = source.run(&[], &[]);
    assert!(run.ok, "{}", run.stderr);
    assert!(run.stderr.contains("build-progress-line"), "{}", run.stderr);
    assert!(run.stderr.contains("build-stdout-line"), "{}", run.stderr);
    assert!(!run.stdout.contains("build-stdout-line"), "{}", run.stdout);
}

#[test]
fn a_dockerfile_between_one_and_sixteen_mebibytes_is_built() {
    let source = Source::new("large-dockerfile");
    let dockerfile = source.path("checkout/images/job/ubuntu-26.04/Dockerfile");
    fs::write(
        &dockerfile,
        format!("FROM scratch\n{}", "# padding\n".repeat(250_000)),
    )
    .unwrap_or_default();
    source.commit_all("large");
    let run = source.run(&[], &[]);
    assert!(run.ok, "{}", run.stderr);
}

#[test]
fn state_planted_by_the_validation_script_never_reaches_the_build() {
    let source = Source::new("planted-state");
    source.script(concat!(
        "dir=\"$HOME/../docker-config/cli-plugins\"\n",
        "mkdir -p \"$dir\"\n",
        "printf '#!/bin/sh\\necho evil >> \"$FAKE_DIR/evil.ran\"\\n' > \"$dir/docker-buildx\"\n",
        "chmod +x \"$dir/docker-buildx\""
    ));
    let run = source.run(&[], &[]);
    assert!(run.ok, "{}", run.stderr);
    assert_eq!(source.text("evil.ran"), "");
    assert!(source.text("plugin.ran").contains("build"));
}

#[test]
fn a_dockerfile_with_leading_and_trailing_whitespace_is_compared_byte_for_byte() {
    let source = Source::new("dockerfile-whitespace");
    let dockerfile = source.path("checkout/images/job/ubuntu-26.04/Dockerfile");
    fs::write(&dockerfile, "\n  FROM scratch \t\n\n").unwrap_or_default();
    source.commit_all("whitespace");
    let run = source.run(&[], &[]);
    assert!(run.ok, "{}", run.stderr);
}
