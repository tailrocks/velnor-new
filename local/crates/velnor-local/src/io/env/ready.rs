// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::fs;

use super::{Addresses, Engine, Environment};
use crate::authority::AGENTS_PATH;
use crate::cli::EnvironmentArgs;
use crate::io::probe::wait_for;
use crate::io::proxy::Proxy;

const READY_TIMEOUT: Duration = Duration::from_secs(60);

impl<E: Engine> Environment<E> {
    pub(in crate::io) async fn published(&self, role: &str) -> io::Result<String> {
        let container = format!("{}-{role}", self.name);
        let ports = self.docker("port", &[&container, "81/tcp"]).await?;
        let address = ports
            .lines()
            .find_map(|line| line.trim().parse::<SocketAddr>().ok());
        address
            .map(|address| address.to_string())
            .ok_or_else(|| io::Error::other(format!("{role} publishes no port: {ports}")))
    }

    pub(in crate::io) async fn published_if_present(
        &self,
        role: &str,
    ) -> io::Result<Option<String>> {
        let filter = format!("name=^/{}-{role}$", self.name);
        let listed = self.docker("ps -aq --filter", &[&filter]).await?;
        if listed.trim().is_empty() {
            return Ok(None);
        }
        self.published(role).await.map(Some)
    }

    pub(super) async fn ready(&self, args: &EnvironmentArgs) -> io::Result<Addresses> {
        let token = fs::read_to_string(self.dir.join("keys/control")).await?;
        let stub = self.published("api").await?;
        let fork = if args.fork_runners > 0 {
            Some(Proxy::authorized(
                &self.published("fork-cache").await?,
                &token,
            ))
        } else {
            None
        };
        let addresses = Addresses {
            cache: Proxy::authorized(&self.published("cache").await?, &token),
            fork,
            stub: Proxy::authorized(&stub, &token),
            stub_url: format!("http://{stub}"),
            token,
        };
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
        let sides: Vec<_> = [("", &addresses.cache, args.containers(args.runners))]
            .into_iter()
            .chain(
                addresses
                    .fork
                    .as_ref()
                    .map(|fork| ("fork-", fork, args.containers(args.fork_runners))),
            )
            .collect();
        for (prefix, proxy, _) in &sides {
            let what = format!("{prefix}runner.server");
            wait_for(proxy, AGENTS_PATH, deadline, &what).await?;
        }
        for (prefix, proxy, runners) in sides {
            for number in 1..=runners {
                let path = format!(
                    "/_apis/v1/Message/isagentonline?name={}-{prefix}runner-{number}",
                    self.name
                );
                let what = format!("{prefix}runner {number}");
                wait_for(proxy, &path, deadline, &what).await?;
            }
        }
        Ok(addresses)
    }
}
