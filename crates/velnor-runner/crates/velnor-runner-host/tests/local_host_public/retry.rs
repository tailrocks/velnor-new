// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::HostError;
use velnor_runner_host::local::supervise;

use super::scenario::Scenario;
use super::support::TestResult;

#[tokio::test]
async fn local_pair_docker_failures_are_retried_three_times_before_the_host_gives_up()
-> TestResult<()> {
    let scenario = Scenario::new().await?;
    let mut plan = scenario.plan(1, 0)?;
    plan.runner_image = format!("sha256:{}", "0".repeat(64));
    let docker = scenario.fixture.docker.clone();
    let state = scenario.state.path().to_path_buf();
    let ended = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        supervise(docker, plan, &state, std::future::pending()),
    )
    .await;
    let events = scenario.fake().events();
    scenario.finish().await?;
    let mints = events
        .iter()
        .filter(|event| event.starts_with("mint:"))
        .count();
    assert_eq!(ended?, Err(HostError::Docker));
    assert_eq!(mints, 2 * 3, "{events:?}");
    for agent in ["agent-1", "agent-2", "agent-3"] {
        let deleted = format!("delete:trusted:{agent}");
        assert!(events.contains(&deleted), "{events:?}");
    }
    Ok(())
}
