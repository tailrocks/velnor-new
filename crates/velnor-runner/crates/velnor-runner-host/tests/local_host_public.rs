// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Instant;

use bollard::models::ContainerInspectResponse;
use bollard::query_parameters::ListContainersOptions;
use velnor_runner_host::HostError;
use velnor_runner_host::local::{LoopbackAuthority, check_state, run_local};

#[expect(
    dead_code,
    reason = "Each test binary uses part of the shared pair fixtures."
)]
#[path = "local_pair_public/support.rs"]
mod support;
use support::{TestResult, labelled};

#[path = "local_host_public/authority.rs"]
mod authority;
#[path = "local_host_public/journal.rs"]
mod journal;
#[path = "local_host_public/proxy.rs"]
mod proxy;
#[path = "local_host_public/refusals.rs"]
mod refusals;
#[path = "local_host_public/retry.rs"]
mod retry;
#[path = "local_host_public/scenario.rs"]
mod scenario;
#[path = "local_host_public/sweep.rs"]
mod sweep;
use authority::{JIT, Run, TOKEN};
use scenario::{
    Live, Scenario, finish_job, live_runners, read_challenge, request, submit, wait_for_decision,
    wait_for_runner,
};

fn dispatched(job: &str) -> Run {
    Run {
        job: job.to_owned(),
        ledger: Some("1-push".to_owned()),
        ..Run::default()
    }
}

fn undispatched(refusal: &str) -> Run {
    Run {
        job: "ci".to_owned(),
        refusal: Some(refusal.to_owned()),
        ..Run::default()
    }
}

async fn runner(scenario: &Scenario, side: &str, seen: &[String]) -> TestResult<Live> {
    let previous: Vec<&str> = seen.iter().map(String::as_str).collect();
    wait_for_runner(
        &scenario.fixture.docker,
        &scenario.fixture.environment,
        side,
        &previous,
    )
    .await
}

async fn outcome(scenario: &Scenario, live: &Live, run_id: u64) -> TestResult<String> {
    let docker = &scenario.fixture.docker;
    let challenge = read_challenge(docker, &live.runner).await?;
    submit(docker, &live.runner, &request(&challenge, run_id, "ci")?).await?;
    let decision = wait_for_decision(docker, &live.runner).await?;
    finish_job(docker, &live.runner).await?;
    Ok(decision
        .deny_code
        .unwrap_or_else(|| format!("allow:{}", decision.native_job_id.unwrap_or_default())))
}

async fn decide_jobs(scenario: &Scenario) -> TestResult<(Vec<String>, u64)> {
    let mut outcomes = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut seconds = 0;
    for (index, run_id) in [1, 2, 3, 4, 5, 1].into_iter().enumerate() {
        let live = runner(scenario, "trusted", &seen).await?;
        seen.push(live.agent.clone());
        let unreachable = index == 5;
        if unreachable {
            scenario.fake().reachable.store(false, Ordering::SeqCst);
        }
        let started = Instant::now();
        let outcome = outcome(scenario, &live, run_id).await?;
        if unreachable {
            seconds = started.elapsed().as_secs();
            scenario.fake().reachable.store(true, Ordering::SeqCst);
        }
        println!("{} run {run_id}: {outcome}", live.agent);
        outcomes.push(outcome);
    }
    Ok((outcomes, seconds))
}

#[tokio::test]
async fn local_job_start_check_decides_each_job_against_the_authority() -> TestResult<()> {
    let mut scenario = Scenario::new().await?;
    let plan = scenario.plan(1, 0)?;
    let fake = scenario.fake();
    fake.dispatch("trusted", 1, dispatched("ci"));
    fake.dispatch("trusted", 3, undispatched("not_dispatched"));
    fake.dispatch("trusted", 4, undispatched("wrong_side"));
    fake.dispatch("trusted", 5, dispatched("deploy"));
    scenario.start(plan);
    let decided = decide_jobs(&scenario).await;
    scenario.fake().reachable.store(true, Ordering::SeqCst);
    let decisions = scenario.decisions();
    let unauthorized = scenario.fake().unauthorized();
    scenario.finish().await?;
    let (outcomes, seconds) = decided?;
    assert_eq!(
        outcomes,
        [
            "allow:1:1:ci",
            "run_unknown",
            "not_dispatched",
            "wrong_side",
            "request_mismatch",
            "authority_unreachable"
        ]
    );
    assert!(seconds < 120);
    assert_eq!(unauthorized, 0);
    let reasons: Vec<&str> = decisions
        .iter()
        .map(|line| line["reason"].as_str().unwrap_or("allow"))
        .collect();
    assert_eq!(
        reasons,
        [
            "allow",
            "run_unknown",
            "not_dispatched",
            "wrong_side",
            "request_mismatch",
            "authority_unreachable"
        ]
    );
    assert_eq!(decisions[0]["ledger"], "1-push");
    assert_eq!(decisions[0]["pair"], "trusted-1");
    Ok(())
}

fn position(events: &[String], prefix: &str) -> Option<usize> {
    events.iter().position(|event| event.starts_with(prefix))
}

async fn networks_of(scenario: &Scenario, side: &str) -> TestResult<Vec<String>> {
    let filters = HashMap::from([(
        "label".to_owned(),
        vec![
            format!("velnor-local={}", scenario.fixture.environment),
            format!("velnor-local.side={side}"),
            "velnor-local.role=dind".to_owned(),
        ],
    )]);
    let found = scenario
        .fixture
        .docker
        .list_containers(Some(ListContainersOptions {
            filters: Some(filters),
            ..Default::default()
        }))
        .await?;
    let id = found
        .first()
        .and_then(|dind| dind.id.clone())
        .ok_or("no dind")?;
    let inspected = scenario.fixture.docker.inspect_container(&id, None).await?;
    Ok(inspected
        .network_settings
        .and_then(|settings| settings.networks)
        .map(|networks| networks.into_keys().collect())
        .unwrap_or_default())
}

#[tokio::test]
async fn local_pairs_run_one_job_and_are_replaced_after_their_agent_is_deleted() -> TestResult<()> {
    let mut scenario = Scenario::new().await?;
    let plan = scenario.plan(1, 1)?;
    scenario.start(plan);
    let run = async {
        let first = runner(&scenario, "trusted", &[]).await?;
        let fork = runner(&scenario, "untrusted", &[]).await?;
        let fork_networks = networks_of(&scenario, "untrusted").await?;
        finish_job(&scenario.fixture.docker, &first.runner).await?;
        let second = runner(&scenario, "trusted", std::slice::from_ref(&first.agent)).await?;
        TestResult::Ok((first, fork, second, fork_networks))
    }
    .await;
    let stopped = scenario.stop().await?;
    let events = scenario.fake().events();
    let left = labelled(&scenario.fixture.docker, &scenario.fixture.environment).await?;
    let fork_network = scenario.fork_network.clone();
    scenario.finish().await?;
    let (first, fork, second, fork_networks) = run?;
    assert_eq!(stopped, Ok(()));
    assert_eq!(left, (0, 0));
    assert_eq!(fork_networks, [fork_network]);
    let deleted = position(&events, &format!("delete:trusted:{}", first.agent));
    let replacement = position(&events, &format!("mint:{}:", second.agent));
    assert!(deleted < replacement, "{events:?}");
    assert!(
        events[replacement.unwrap_or_default()].contains("live_trusted=0"),
        "{events:?}"
    );
    for agent in [&second.agent, &fork.agent] {
        let side = if *agent == fork.agent {
            "untrusted"
        } else {
            "trusted"
        };
        assert!(
            position(&events, &format!("delete:{side}:{agent}")).is_some(),
            "{events:?}"
        );
    }
    Ok(())
}

fn secrets_in(inspected: &ContainerInspectResponse) -> String {
    let config = inspected.config.as_ref();
    format!(
        "{:?} {:?} {:?} {:?}",
        config.and_then(|config| config.env.clone()),
        config.and_then(|config| config.cmd.clone()),
        config.and_then(|config| config.entrypoint.clone()),
        inspected.mounts
    )
}

#[tokio::test]
async fn local_pairs_hold_neither_the_authority_address_nor_the_token() -> TestResult<()> {
    let mut scenario = Scenario::new().await?;
    let plan = scenario.plan(1, 1)?;
    let address = scenario.served.address.to_string();
    scenario.start(plan);
    let run = async {
        runner(&scenario, "trusted", &[]).await?;
        runner(&scenario, "untrusted", &[]).await?;
        let mut shown = Vec::new();
        for side in ["trusted", "untrusted"] {
            for live in live_runners(
                &scenario.fixture.docker,
                &scenario.fixture.environment,
                side,
            )
            .await?
            {
                let runner = scenario
                    .fixture
                    .docker
                    .inspect_container(&live.runner, None)
                    .await?;
                shown.push(secrets_in(&runner));
            }
        }
        TestResult::Ok(shown)
    }
    .await;
    scenario.finish().await?;
    let shown = run?;
    assert_eq!(shown.len(), 2);
    for text in shown {
        let port = address.rsplit(':').next().unwrap_or_default();
        assert!(!text.contains(&address) && !text.contains(TOKEN) && !text.contains(JIT));
        assert!(!text.contains(&format!("127.0.0.1:{port}")));
    }
    Ok(())
}

#[test]
fn local_authority_must_be_loopback_http() {
    for refused in [
        "https://api.github.com",
        "http://api.github.com:80",
        "http://10.0.0.5:80",
        "http://localhost:53114",
        "http://127.0.0.2:53114",
        "http://0.0.0.0:53114",
        "http://127.0.0.1",
        "http://127.0.0.1:0",
        "http://127.0.0.1:53114/",
        "http://user@127.0.0.1:53114",
        "http://[::ffff:127.0.0.1]:53114",
        "http://[::2]:53114",
        "127.0.0.1:53114",
        "",
    ] {
        assert_eq!(
            LoopbackAuthority::parse(refused),
            Err(HostError::Config),
            "{refused}"
        );
    }
    for accepted in ["http://127.0.0.1:53114", "http://[::1]:53114"] {
        assert!(LoopbackAuthority::parse(accepted).is_ok(), "{accepted}");
    }
}

#[tokio::test]
async fn local_host_refuses_a_remote_authority_before_any_container_starts() -> TestResult<()> {
    let docker = support::docker()?;
    let environment = format!("test-{}", support::suffix()?);
    let state = tempfile::tempdir()?;
    std::fs::write(state.path().join("velnor-local"), "")?;
    let image = format!("sha256:{}", "a".repeat(64));
    let mut outcomes = Vec::new();
    for authority in ["https://api.github.com", "http://10.0.0.5:80"] {
        let plan = format!(
            "{{\"environment\":\"{environment}\",\"authority\":\"{authority}\",\"token\":\"{TOKEN}\",\"docker\":\"unix:///var/run/docker.sock\",\"platform\":\"{}\",\"runner_image\":\"{image}\",\"dind_image\":\"{image}\",\"limits\":{{\"runner_cpu_millicores\":1500,\"runner_memory_bytes\":1073741824,\"dind_cpu_millicores\":500,\"dind_memory_bytes\":1073741824}},\"sides\":[{{\"side\":\"trusted\",\"network\":\"velnor-local-x\",\"pairs\":1}}]}}",
            support::platform()
        );
        outcomes.push(run_local(state.path(), plan.as_bytes()));
    }
    assert_eq!(outcomes, [Err(HostError::Config), Err(HostError::Config)]);
    assert_eq!(labelled(&docker, &environment).await?, (0, 0));
    Ok(())
}

#[test]
fn local_state_must_be_a_marked_directory_without_daemon_files() -> TestResult<()> {
    let marked = |extra: &[&str]| -> TestResult<Result<(), HostError>> {
        let state = tempfile::tempdir()?;
        std::fs::write(state.path().join("velnor-local"), "")?;
        for name in extra {
            std::fs::write(state.path().join(name), "")?;
        }
        Ok(check_state(state.path()))
    };
    let unmarked = tempfile::tempdir()?;
    assert_eq!(check_state(unmarked.path()), Err(HostError::Config));
    let directory = tempfile::tempdir()?;
    std::fs::create_dir(directory.path().join("velnor-local"))?;
    assert_eq!(check_state(directory.path()), Err(HostError::Config));
    assert_eq!(marked(&[])?, Ok(()));
    assert_eq!(marked(&["decisions.jsonl", "plan.json"])?, Ok(()));
    for extra in [
        "host.toml",
        "launch.db",
        "launch.db-wal",
        "journal.db",
        "daemon.lock",
        "admission.lock",
    ] {
        assert_eq!(marked(&[extra])?, Err(HostError::Config), "{extra}");
    }
    Ok(())
}
