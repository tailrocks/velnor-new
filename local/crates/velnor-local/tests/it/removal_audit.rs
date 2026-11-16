// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use velnor_local::io::env::{DockerCli, Engine, Environment};

use crate::support::temp_dir;

struct Docker {
    endpoint: String,
    removals: AtomicUsize,
    fail_at: usize,
}

impl Docker {
    fn selected(fail_at: usize) -> io::Result<Option<Self>> {
        let endpoint = match std::env::var("VELNOR_TEST_DOCKER_HOST") {
            Ok(endpoint) => endpoint,
            Err(std::env::VarError::NotPresent) => {
                eprintln!(
                    "SKIP Docker integration test: set VELNOR_TEST_DOCKER_HOST to an isolated test daemon"
                );
                return Ok(None);
            }
            Err(error) => return Err(io::Error::other(error)),
        };
        let unix = endpoint.strip_prefix("unix://").is_some_and(|path| {
            Path::new(path).is_absolute() && path.len() > 1 && !path.chars().any(char::is_control)
        });
        let tcp = endpoint.strip_prefix("tcp://").is_some_and(|address| {
            format!("http://{address}")
                .parse::<hyper::Uri>()
                .is_ok_and(|uri| {
                    uri.authority().is_some_and(|authority| {
                        authority.as_str() == address
                            && !authority.host().is_empty()
                            && authority.port_u16().is_some_and(|port| port > 0)
                    }) && !address.contains('@')
                })
        });
        if !unix && !tcp {
            return Err(io::Error::other(
                "VELNOR_TEST_DOCKER_HOST must be an explicit unix:///socket or tcp://host:port endpoint",
            ));
        }
        Ok(Some(Self {
            endpoint,
            removals: AtomicUsize::new(0),
            fail_at,
        }))
    }
}

impl Engine for &Docker {
    async fn docker(&self, args: &[&str]) -> io::Result<String> {
        if args.starts_with(&["network", "rm"])
            && self.removals.fetch_add(1, Ordering::SeqCst) == self.fail_at
        {
            return Err(io::Error::other("injected Docker removal failure"));
        }
        DockerCli
            .docker(&[&["-H", &self.endpoint][..], args].concat())
            .await
    }

    fn logged(
        &self,
        _: &[&str],
        _: &Path,
    ) -> impl std::future::Future<Output = io::Result<bool>> + Send {
        std::future::ready(Err(io::Error::other("unused Docker operation")))
    }
}

fn journal(dir: &Path) -> io::Result<Vec<Value>> {
    std::fs::read_to_string(dir.join("cleanup.jsonl"))?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(io::Error::other))
        .collect()
}

#[tokio::test]
async fn environment_down_audits_exact_ids_and_keeps_other_environments() -> io::Result<()> {
    let Some(docker) = Docker::selected(usize::MAX)? else {
        return Ok(());
    };
    let dir = temp_dir("audit-filter");
    let env = Environment::at(&dir, &docker).await?;
    let label = format!("velnor-local={}", env.name);
    let network = (&docker)
        .docker(&["network", "create", "--label", &label, &env.name])
        .await?;
    let volume = (&docker)
        .docker(&["volume", "create", "--label", &label])
        .await?;
    let foreign = format!("{}-foreign", env.name);
    let foreign_label = format!("velnor-local={foreign}");
    (&docker)
        .docker(&["network", "create", "--label", &foreign_label, &foreign])
        .await?;
    let outcome = env.down().await;
    let kept = (&docker)
        .docker(&["network", "inspect", &foreign])
        .await
        .is_ok();
    let gone_network = (&docker)
        .docker(&["network", "inspect", &network])
        .await
        .is_err();
    let gone_volume = (&docker)
        .docker(&["volume", "inspect", &volume])
        .await
        .is_err();
    let records = journal(&dir);
    let _ = (&docker).docker(&["network", "rm", &foreign]).await;
    let _ = (&docker).docker(&["network", "rm", &network]).await;
    if !gone_volume {
        (&docker).docker(&["volume", "rm", &volume]).await?;
    }
    std::fs::remove_dir_all(&dir)?;
    assert_eq!(outcome?, [0, 1, 1]);
    assert!(kept && gone_network && gone_volume);
    let records = records?;
    assert_eq!(records[0]["event"], "intent");
    for record in &records {
        assert_eq!(record["filters"], json!({"label": [label]}));
    }
    assert!(records.iter().any(|line| line["event"] == "removed"
        && line["id"] == network
        && line["resource"] == "network"));
    assert!(records.iter().any(|line| line["event"] == "removed"
        && line["id"] == volume
        && line["resource"] == "volume"));
    assert_eq!(network.len(), 64);
    Ok(())
}

#[tokio::test]
async fn environment_down_preserves_confirmed_removals_after_failure_and_appends_on_retry()
-> io::Result<()> {
    let Some(docker) = Docker::selected(1)? else {
        return Ok(());
    };
    let dir = temp_dir("audit-partial");
    let env = Environment::at(&dir, &docker).await?;
    let label = format!("velnor-local={}", env.name);
    let mut ids = Vec::new();
    for suffix in ["one", "two"] {
        ids.push(
            (&docker)
                .docker(&[
                    "network",
                    "create",
                    "--label",
                    &label,
                    &format!("{}-{suffix}", env.name),
                ])
                .await?,
        );
    }
    let failed = env.down().await;
    let before = journal(&dir)?;
    let removed: Vec<_> = before
        .iter()
        .filter(|line| line["event"] == "removed")
        .collect();
    let gone = if let Some(record) = removed.first() {
        (&docker)
            .docker(&[
                "network",
                "inspect",
                record["id"].as_str().unwrap_or_default(),
            ])
            .await
            .is_err()
    } else {
        false
    };
    let retried = env.down().await;
    let after = journal(&dir)?;
    for id in &ids {
        let _ = (&docker).docker(&["network", "rm", id]).await;
    }
    std::fs::remove_dir_all(&dir)?;
    assert!(failed.is_err_and(|error| error.to_string().contains("injected")));
    assert_eq!(removed.len(), 1);
    assert!(gone);
    assert_eq!(retried?, [0, 0, 1]);
    assert_eq!(&after[..before.len()], &before);
    for id in ids {
        assert_eq!(
            after
                .iter()
                .filter(|line| line["event"] == "removed" && line["id"] == id)
                .count(),
            1
        );
    }
    Ok(())
}

#[tokio::test]
async fn environment_down_refuses_to_delete_without_a_writable_audit() -> io::Result<()> {
    let Some(docker) = Docker::selected(usize::MAX)? else {
        return Ok(());
    };
    let dir = temp_dir("audit-unwritable");
    let env = Environment::at(&dir, &docker).await?;
    let label = format!("velnor-local={}", env.name);
    let volume = (&docker)
        .docker(&["volume", "create", "--label", &label])
        .await?;
    std::fs::create_dir(dir.join("cleanup.jsonl"))?;
    let failed = env.down().await;
    let kept = (&docker)
        .docker(&["volume", "inspect", &volume])
        .await
        .is_ok();
    if kept {
        (&docker).docker(&["volume", "rm", &volume]).await?;
    }
    std::fs::remove_dir_all(&dir)?;
    assert!(failed.is_err());
    assert!(kept);
    Ok(())
}
