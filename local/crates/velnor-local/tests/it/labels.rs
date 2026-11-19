// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_local::cli::{EnvironmentArgs, RunnerMode};
use velnor_local::io::env::Environment;

use crate::fake_docker::FakeDocker;
use crate::support::temp_dir;

fn plain(dir: &std::path::Path) -> EnvironmentArgs {
    EnvironmentArgs {
        dir: dir.to_path_buf(),
        runners: 1,
        fork_runners: 0,
        default_branch: "main".to_owned(),
        runner: RunnerMode::Plain,
        velnor_host: Some("velnor-host".into()),
        policy: None,
        label: Vec::new(),
        ..EnvironmentArgs::default()
    }
}

#[test]
fn extra_labels_follow_the_built_in_ones_without_duplicates() {
    let built_in = velnor_local::jit::labels(&[]);
    let extra = |labels: &[&str]| {
        let labels: Vec<String> = labels.iter().map(|label| (*label).to_owned()).collect();
        velnor_local::jit::labels(&labels)
    };
    assert_eq!(
        extra(&["ubuntu-26.04-scale-set", "ci-fast"]),
        format!("{built_in},ubuntu-26.04-scale-set,ci-fast")
    );
    assert_eq!(extra(&["ubuntu-latest"]), built_in);
    assert_eq!(extra(&["a", "a", "ubuntu-24.04"]), format!("{built_in},a"));
}

#[tokio::test]
async fn a_plain_runner_registers_the_extra_labels() {
    let built_in = velnor_local::jit::labels(&[]);
    let dir = temp_dir("environment-labels");
    let docker = FakeDocker::default();
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .with_labels(&["ubuntu-26.04-scale-set".to_owned()]);
    let args = plain(&dir);
    assert!(env.up(&dir, &args).await.is_err());
    let registrations: Vec<String> = docker
        .started()
        .iter()
        .filter(|call| call.iter().any(|arg| arg.contains("config.sh")))
        .map(|call| call.join(" "))
        .collect();
    assert_eq!(registrations.len(), 1, "{registrations:?}");
    assert!(
        registrations[0].contains(&format!("--labels {built_in},ubuntu-26.04-scale-set &&")),
        "{registrations:?}"
    );
}
