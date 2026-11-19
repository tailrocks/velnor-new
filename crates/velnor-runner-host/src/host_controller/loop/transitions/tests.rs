// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{TransitionEffects, apply_config_transition};
use crate::config::{HostConfig, RepositoryConnection};
use crate::error::HostError;

#[derive(Default)]
struct RecordingEffects {
    budgets: usize,
    bound: usize,
    published: Vec<HostConfig>,
}

impl TransitionEffects for RecordingEffects {
    fn configure_shared_budget(
        &mut self,
        _config: &HostConfig,
    ) -> impl Future<Output = Result<(), HostError>> {
        self.budgets += 1;
        std::future::ready(Ok(()))
    }

    fn bind_connection(
        &mut self,
        _binding: &RepositoryConnection,
        _config: &HostConfig,
    ) -> impl Future<Output = Result<(), HostError>> {
        self.bound += 1;
        std::future::ready(Ok(()))
    }

    fn publish_config(&mut self, config: &HostConfig) -> Result<(), HostError> {
        self.published.push(config.clone());
        Ok(())
    }

    fn retire_removed(
        &mut self,
        _binding: &RepositoryConnection,
    ) -> impl Future<Output = Result<bool, HostError>> {
        std::future::ready(Ok(true))
    }
}

fn config(developer_images: &str) -> Result<HostConfig, HostError> {
    HostConfig::parse(&format!(
        "schema = 2\n\
         [[connections]]\n\
         connection_id = \"lab\"\n\
         repository_id = 42\n\
         repository = \"tailrocks/velnor-lab\"\n\
         credential_ref = \"keychain:com.tailrocks.velnor.host/lab\"\n\
         [connections.scale_set]\n\
         name = \"ubuntu-26.04-arm-scale-set\"\n\
         labels = [\"velnor\", \"ubuntu-26.04-arm-scale-set\"]\n\
         [connections.source_policy]\n\
         mode = \"branch_restricted\"\n\
         branches = [\"main\"]\n\
         tag_prefixes = []\n\
         events = [\"push\"]\n\
         pull_request_rules = []\n\
         {developer_images}\
         [host]\n\
         max_jobs = 1\n\
         [host.resources]\n\
         runner_cpu_millicores = 2000\n\
         runner_memory_bytes = 4294967296\n\
         dind_cpu_millicores = 1000\n\
         dind_memory_bytes = 2147483648\n\
         operational_memory_reserve_bytes = 1073741824\n\
         [docker]\n\
         context = \"orbstack\"\n\
         platform = \"linux/arm64\"\n\
         endpoint = \"unix:///var/run/docker.sock\"\n"
    ))
}

fn images(runner: char) -> String {
    format!(
        "[developer_images]\nrunner = \"sha256:{}\"\ndind = \"sha256:{}\"\nresource_probe = \"sha256:{}\"\n",
        runner.to_string().repeat(64),
        "b".repeat(64),
        "c".repeat(64)
    )
}

#[tokio::test]
async fn developer_images_change_only_on_restart() -> Result<(), HostError> {
    let current = config(&images('a'))?;
    let mut same = RecordingEffects::default();
    assert!(
        apply_config_transition(&current, &current.clone(), &mut same)
            .await
            .is_ok()
    );
    assert_eq!(same.published, vec![current.clone()]);
    for next in [config(&images('d'))?, config("")?] {
        let mut effects = RecordingEffects::default();
        assert_eq!(
            apply_config_transition(&current, &next, &mut effects).await,
            Err(HostError::Config)
        );
        assert_eq!(
            (effects.budgets, effects.bound, effects.published.len()),
            (0, 0, 0),
            "a refused reload had effects"
        );
    }
    Ok(())
}
