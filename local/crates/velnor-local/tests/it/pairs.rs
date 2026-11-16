// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use velnor_local::cli::{Cli, Command, EnvironmentArgs, RunnerMode};
use velnor_local::images::{HOST_BINARY, HOST_BUILD, RELEASE_IDENTITY};
use velnor_local::io::env::Environment;
use velnor_local::io::supervisor::write_plan;
use velnor_local::pairs::{Decision, PlanInput, decisions, listening, plan, slots};
use velnor_local::scenario::Outcome;
use velnor_local::side::Side;
use velnor_local::verdict::{Check, conclude, verdict};

use crate::fake_docker::{FakeDocker, after};
use crate::support::temp_dir;

const ADMITTED: &str = r#"{"pair":"trusted-1","side":"trusted","agent":"a1","allow":true,"reason":null,"ledger":"1-push","run_id":1,"attempt":1,"job":"ci","event":"push","ref":"refs/heads/main","sha":"abc"}"#;
const REFUSED: &str = r#"{"pair":"trusted-2","side":"trusted","agent":"a2","allow":false,"reason":"not_dispatched","ledger":null,"run_id":2,"attempt":1,"job":"ci","event":"push","ref":"refs/heads/main","sha":"abc"}"#;
const FORK: &str = r#"{"pair":"untrusted-1","side":"untrusted","agent":"a3","allow":true,"reason":null,"ledger":"3-pull_request","run_id":1,"attempt":1,"job":"lint","event":"pull_request","ref":"refs/pull/1/merge","sha":"def"}"#;
const MALFORMED: &str = r#"{"pair":"trusted-1","side":"trusted","agent":"a4","allow":false,"reason":"request_malformed","ledger":null,"run_id":null,"attempt":null,"job":null,"event":null,"ref":null,"sha":null}"#;

fn outcome(id: u64, trusted: bool) -> Outcome {
    Outcome {
        id,
        workflow: "mbx default".to_owned(),
        event: "push".to_owned(),
        conclusion: "success",
        started: Some(112),
        metrics: Err(String::new()),
        caches: Vec::new(),
        trusted,
    }
}

fn read(lines: &[&str]) -> Vec<Decision> {
    decisions(&lines.join("\n")).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn the_plan_names_the_loopback_authority_the_image_ids_and_only_sides_with_pairs() {
    let sides = [
        (Side::Trusted, "velnor-local-ab".to_owned(), 2),
        (Side::Untrusted, "velnor-local-ab-fork".to_owned(), 0),
    ];
    let input = PlanInput {
        environment: "velnor-local-ab",
        authority: "127.0.0.1:5311",
        token: "t0k3n",
        docker: "unix:///var/run/docker.sock",
        runner_image: "sha256:aa",
        dind_image: "sha256:bb",
        sides: &sides,
    };
    let planned = plan(&input);
    assert_eq!(planned["authority"], "http://127.0.0.1:5311");
    assert_eq!(planned["environment"], "velnor-local-ab");
    assert_eq!(planned["token"], "t0k3n");
    assert_eq!(planned["platform"], "linux/arm64");
    assert_eq!(planned["runner_image"], "sha256:aa");
    assert_eq!(planned["dind_image"], "sha256:bb");
    assert_eq!(
        planned["sides"],
        serde_json::json!([{"side": "trusted", "network": "velnor-local-ab", "pairs": 2}])
    );
    let both = [
        (Side::Trusted, "n".to_owned(), 1),
        (Side::Untrusted, "n-fork".to_owned(), 1),
    ];
    let planned = plan(&PlanInput {
        sides: &both,
        ..input
    });
    assert_eq!(planned["sides"][1]["side"], "untrusted");
    assert_eq!(planned["sides"][1]["network"], "n-fork");
}

#[test]
fn the_pairs_are_ready_only_when_every_slot_listens() {
    let slots = slots(2, 1);
    assert_eq!(slots, ["trusted-1", "trusted-2", "untrusted-1"]);
    let log = "pair trusted-2 listening\npair trusted-1 listening\npair untrusted-1 removed\n";
    assert_eq!(listening(log, &slots), ["trusted-1", "trusted-2"]);
    let log = format!("{log}pair untrusted-1 listening\n");
    assert_eq!(listening(&log, &slots).len(), slots.len());
}

#[test]
fn a_slot_that_logged_listening_and_then_exited_is_not_counted_ready() {
    let slots = slots(2, 0);
    let log = "pair trusted-1 listening\npair trusted-2 listening\npair trusted-1 removed\n";
    assert_eq!(listening(log, &slots), ["trusted-2"]);
    let log = format!("{log}pair trusted-1 listening\n");
    assert_eq!(listening(&log, &slots), ["trusted-1", "trusted-2"]);
}

#[test]
fn each_decision_reads_as_admitted_with_its_event_or_refused_with_its_reason() {
    let read = read(&[ADMITTED, REFUSED, "", MALFORMED]);
    let lines: Vec<String> = read.iter().map(Decision::line).collect();
    assert_eq!(
        lines,
        [
            "job ci: pair trusted-1, admitted (event 1-push)",
            "job ci: pair trusted-2, REFUSED at job start: not_dispatched",
            "job unknown: pair trusted-1, REFUSED at job start: request_malformed",
        ]
    );
    assert!(decisions("{not json").is_err());
}

#[test]
fn a_pair_verdict_puts_each_job_under_its_run_and_side_and_lists_the_rest() {
    let check = Check::Pairs(read(&[ADMITTED, FORK, MALFORMED]));
    let text = verdict("warm", &[outcome(1, true)], (100, 9.8), &check);
    let lines: Vec<&str> = text.lines().collect();
    let run = lines
        .iter()
        .position(|line| line.contains("run 1 mbx default (push): success"))
        .unwrap_or_else(|| unreachable!("{text}"));
    assert_eq!(
        lines[run + 1],
        "velnor-local warm:     job ci: pair trusted-1, admitted (event 1-push)"
    );
    assert!(text.contains("velnor-local warm: Job-start decisions outside these runs:\nvelnor-local warm:   untrusted side, run 1: job lint: pair untrusted-1, admitted (event 3-pull_request)\nvelnor-local warm:   trusted side, no run: job unknown: pair trusted-1, REFUSED at job start: request_malformed\n"), "{text}");
    assert!(
        text.contains("first job started 12 s after up began"),
        "{text}"
    );
    for part in [
        "executed: official runner 2.338.0 in velnor-host pairs (private Docker, limits, job-start check)",
        "Stood in: jobs and job-start facts by runner.server and the driver's ledger",
        "Not exercised: scale set acquire, GitHub JIT, journal, GitHub runner deletion proof",
    ] {
        assert!(text.contains(part), "{part}\n{text}");
    }
    assert!(!text.contains("plain containers"), "{text}");
    assert!(check.refused());
    assert!(!Check::Pairs(read(&[ADMITTED, FORK])).refused());
}

#[test]
fn a_plain_verdict_says_there_is_no_job_start_check() {
    let text = verdict("warm", &[outcome(1, true)], (100, 9.8), &Check::Plain);
    assert!(
        text.contains("job-start check: none (plain containers)"),
        "{text}"
    );
    assert!(!text.contains("pair trusted"), "{text}");
    assert!(!Check::Plain.refused());
}

#[test]
fn pair_mode_is_the_default_and_names_the_velnor_host_it_runs() {
    let parsed = Cli::try_parse_from(["velnor-local", "up", "--dir", "s", "--repo", "r"]);
    let Ok(Cli {
        command: Command::Up(up),
    }) = parsed
    else {
        unreachable!("{parsed:?}")
    };
    assert_eq!(up.environment.runner, RunnerMode::Pair);
    assert_eq!(up.environment.velnor_host, None);
    assert_eq!(up.environment.containers(2), 0);
    let plain = ["velnor-local", "run", "warm", "--dir", "s", "--repo", "r"];
    let parsed = Cli::try_parse_from(plain.into_iter().chain(["--runner", "plain"]));
    let Ok(Cli {
        command: Command::Run(run),
    }) = parsed
    else {
        unreachable!("{parsed:?}")
    };
    assert_eq!(run.environment.runner, RunnerMode::Plain);
    assert_eq!(run.environment.containers(2), 2);
}

#[test]
fn the_velnor_host_build_has_local_mode_and_no_release_identity() {
    assert!(HOST_BUILD.contains(&"--features=local-mode"));
    assert!(HOST_BUILD.contains(&"--release"));
    assert!(HOST_BUILD.contains(&"velnor-host"));
    assert!(HOST_BINARY.ends_with("release/velnor-host"));
    assert!(RELEASE_IDENTITY.contains(&"VELNOR_RUNNER_BUILD_SOURCE_SHA"));
    assert!(RELEASE_IDENTITY.contains(&"VELNOR_RUNNER_BUILD_HELPER_SHA256"));
}

#[tokio::test]
async fn pair_mode_starts_no_runner_container_and_down_removes_labelled_volumes() {
    let dir = temp_dir("pairs-environment");
    let docker = FakeDocker::default();
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(env.name.starts_with("velnor-local-"), "{}", env.name);
    assert_eq!(env.network(Side::Untrusted), format!("{}-fork", env.name));
    let args = EnvironmentArgs {
        dir: dir.clone(),
        runners: 2,
        fork_runners: 1,
        default_branch: "main".to_owned(),
        runner: RunnerMode::Pair,
        velnor_host: Some(HOST_BINARY.into()),
    };
    assert!(env.up(&dir, &args).await.is_err());
    let started = docker.started();
    let names: Vec<&str> = started
        .iter()
        .flat_map(|call| after(call, "--name"))
        .collect();
    assert!(
        names.contains(&format!("{}-standin", env.name).as_str()),
        "{names:?}"
    );
    assert!(
        !names.iter().any(|name| name.contains("runner-")),
        "{names:?}"
    );
    let label = format!("velnor-local={}", env.name);
    docker
        .labelled
        .lock()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .extend([
            ("volume:w1-metadata".to_owned(), label.clone()),
            ("container:w1-dind".to_owned(), label),
            ("volume:other".to_owned(), "velnor-local=test-x".to_owned()),
        ]);
    let removed = env.down().await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(removed, [1, 1, 0]);
    assert_eq!(docker.left(), ["volume:other"]);
}

#[test]
fn every_event_names_a_positive_repository_id_for_the_job_start_hook() {
    use velnor_local::event::{NO_COMMIT, Push, Repository};
    let repository = |full_name| Repository {
        full_name,
        default_branch: "main",
    };
    let push = Push {
        branch: "main",
        before: NO_COMMIT,
        after: "a77d1f28282df9e2cd35df73548b9b9362d9549e",
        commits: &[],
    };
    let id = |full_name| repository(full_name).push(&push)["repository"]["id"].as_u64();
    let lab = id("tailrocks/velnor-lab");
    assert!(lab.is_some_and(|id| id > 0), "{lab:?}");
    assert_eq!(lab, id("tailrocks/velnor-lab"));
    assert_ne!(lab, id("tailrocks/velnor-new"));
}

#[test]
fn a_refused_job_fails_the_run_even_when_every_run_succeeded() {
    let passed = [outcome(1, false)];
    let refused = Check::Pairs(read(&[ADMITTED, REFUSED]));
    assert_eq!(
        conclude(&refused, &passed),
        Err("a job was refused at job start")
    );
    assert_eq!(conclude(&Check::Pairs(read(&[ADMITTED])), &passed), Ok(()));
    assert_eq!(conclude(&Check::Plain, &passed), Ok(()));
}

#[test]
fn a_run_that_did_not_succeed_fails_the_run() {
    let mut failed = outcome(1, false);
    failed.conclusion = "failure";
    assert_eq!(
        conclude(&Check::Plain, &[failed]),
        Err("a run did not succeed")
    );
}

#[tokio::test]
async fn the_plan_file_holding_the_authority_token_is_private_to_its_owner() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("plan-mode");
    write_plan(&dir, "{}")
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mode = std::fs::metadata(dir.join("plan.json"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(
        std::fs::read_to_string(dir.join("plan.json")).unwrap_or_default(),
        "{}"
    );
}

fn installed(root: &std::path::Path, mode: u32) -> (std::path::PathBuf, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let exe = root.join("local/target/release/velnor-local");
    let host = root.join(HOST_BINARY);
    for path in [&exe, &host] {
        let parent = path.parent().unwrap_or_else(|| unreachable!());
        std::fs::create_dir_all(parent).unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(path, "#!/bin/sh\n").unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    (exe, host)
}

#[tokio::test]
async fn the_default_velnor_host_is_the_one_built_beside_the_installed_executable() {
    let root = temp_dir("pairs-host-installed");
    let (exe, host) = installed(&root, 0o755);
    assert_eq!(
        velnor_local::io::supervisor::default_host(&exe).await.ok(),
        Some(host)
    );
}

#[tokio::test]
async fn a_velnor_host_planted_in_the_working_directory_is_never_the_default() {
    let planted = temp_dir("pairs-host-planted");
    installed(&planted, 0o755);
    let elsewhere = temp_dir("pairs-host-elsewhere");
    let exe = elsewhere.join("bin/velnor-local");
    std::fs::create_dir_all(exe.parent().unwrap_or_else(|| unreachable!()))
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(&exe, "").unwrap_or_else(|e| unreachable!("{e}"));
    let resolved = velnor_local::io::supervisor::default_host(&exe).await;
    assert!(resolved.is_err(), "{resolved:?}");
}

#[tokio::test]
async fn a_default_velnor_host_that_others_can_write_is_refused() {
    let root = temp_dir("pairs-host-writable");
    let (exe, _) = installed(&root, 0o775);
    let resolved = velnor_local::io::supervisor::default_host(&exe).await;
    assert!(resolved.is_err(), "{resolved:?}");
}

#[test]
fn the_cli_never_picks_a_velnor_host_planted_in_its_working_directory() {
    let cwd = temp_dir("pairs-cli-cwd");
    let planted = cwd.join(HOST_BINARY);
    let installed = temp_dir("pairs-cli-installed").join("bin/velnor-local");
    for dir in [planted.parent(), installed.parent()] {
        std::fs::create_dir_all(dir.unwrap_or_else(|| unreachable!()))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    std::fs::write(&planted, "#!/bin/sh\n").unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::set_permissions(&planted, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::copy(env!("CARGO_BIN_EXE_velnor-local"), &installed)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let output = std::process::Command::new(&installed)
        .current_dir(&cwd)
        .args(["up", "--repo", ".", "--dir"])
        .arg(cwd.join("state"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains(&format!("no {HOST_BINARY} in any folder above")),
        "{stderr}"
    );
}
