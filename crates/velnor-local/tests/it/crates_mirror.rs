// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_local::cli::{EnvironmentArgs, RunnerMode};
use velnor_local::io::env::Environment;

use crate::fake_docker::{FakeDocker, after};
use crate::support::temp_dir;

#[tokio::test]
async fn every_plain_runner_turns_the_crates_mirror_off_and_nothing_else_does() {
    let dir = temp_dir("environment-mirror-off");
    let docker = FakeDocker::default();
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let args = EnvironmentArgs {
        dir: dir.clone(),
        fork_runners: 1,
        runner: RunnerMode::Plain,
        velnor_host: Some("velnor-host".into()),
        ..EnvironmentArgs::default()
    };
    assert!(env.up(&dir, &args).await.is_err());
    let started = docker.started();
    let mut runners = 0;
    for call in &started {
        let name = after(call, "--name")[0];
        let is_runner = name.contains("-runner-");
        runners += usize::from(is_runner);
        assert_eq!(
            after(call, "-e").contains(&"VELNOR_CRATES_MIRROR=off"),
            is_runner,
            "{name}"
        );
        assert_eq!(
            after(call, "-e").contains(&"MISE_SKILLS_FETCH=false"),
            is_runner,
            "{name}"
        );
    }
    assert_eq!(runners, 3);
}
