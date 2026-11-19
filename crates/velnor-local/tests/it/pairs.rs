// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod verdicts;

use clap::Parser;
use velnor_local::cli::{Cli, Command, EnvironmentArgs, RunnerMode};
use velnor_local::images::{HOST_BINARY, HOST_BUILD, RELEASE_IDENTITY};
use velnor_local::io::env::Environment;
use velnor_local::io::supervisor::write_plan;
use velnor_local::pairs::{PlanInput, listening, plan, slots};
use velnor_local::side::Side;

use crate::fake_docker::{FakeDocker, after};
use crate::support::temp_dir;

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
        ..PlanInput::default()
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
        fork_runners: 1,
        velnor_host: Some(HOST_BINARY.into()),
        ..EnvironmentArgs::default()
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
        id: None,
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

#[test]
fn label_takes_comma_separated_and_repeated_values_and_refuses_what_a_shell_would_split() {
    let up = |labels: &[&str]| {
        let mut args = vec!["velnor-local", "up", "--dir", "s", "--repo", "r"];
        for label in labels {
            args.extend(["--label", label]);
        }
        Cli::try_parse_from(args)
    };
    let Ok(Cli {
        command: Command::Up(parsed),
    }) = up(&["a,b", "c"])
    else {
        unreachable!("labels were refused")
    };
    assert_eq!(parsed.environment.label, ["a", "b", "c"]);
    let Ok(Cli {
        command: Command::Up(none),
    }) = up(&[])
    else {
        unreachable!("no labels were refused")
    };
    assert_eq!(none.environment.label, Vec::<String>::new());
    for bad in ["x y", "x;y", "", "a,,b", "$HOME", "x'y"] {
        let refused = up(&[bad]);
        let Err(error) = refused else {
            unreachable!("{bad:?} was accepted")
        };
        assert!(
            error.to_string().contains("runner label"),
            "{bad:?}: {error}"
        );
    }
}

#[test]
fn the_plan_names_the_local_velnor_ci_only_when_one_is_given() {
    let sides = [(Side::Trusted, "n".to_owned(), 1)];
    let mut input = PlanInput {
        environment: "velnor-local-ab",
        authority: "127.0.0.1:5311",
        token: "t0k3n",
        docker: "unix:///var/run/docker.sock",
        runner_image: "sha256:aa",
        dind_image: "sha256:bb",
        sides: &sides,
        ..PlanInput::default()
    };
    assert!(plan(&input).get("local_ci").is_none());
    input.local_ci = Some("/x/velnor-ci");
    assert_eq!(plan(&input)["local_ci"], "/x/velnor-ci");
}
