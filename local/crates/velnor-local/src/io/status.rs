// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::Path;
use std::time::SystemTime;

use tokio::fs;

use super::env::{DockerCli, Environment};
use super::supervisor::authority_running;
use crate::pairs::{DECISIONS, decisions};
use crate::status::{Snapshot, last_refusal, pair_states, render};

async fn text(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path).await {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) async fn run(dir: &Path) -> io::Result<()> {
    let dir = fs::canonicalize(dir).await?;
    let env = Environment::at(&dir, DockerCli).await?;
    let authority = dir.join("authority");
    let address = authority.join("address");
    let running = authority_running(&dir).await?;
    let Some(listening) = text(&address).await?.filter(|_| running) else {
        return Err(io::Error::other(format!(
            "{} is not up: no running authority in {}; start it with `velnor-local up --dir {}`",
            env.name,
            dir.display(),
            dir.display()
        )));
    };
    let up = fs::metadata(&address).await?.modified()?;
    let ledger = text(&authority.join(crate::io::authority::LEDGER)).await?;
    let runner = dir.join("runner");
    let plan = text(&runner.join("plan.json")).await?;
    let log = text(&runner.join("velnor-host.log"))
        .await?
        .unwrap_or_default();
    let judged = text(&runner.join(DECISIONS)).await?.unwrap_or_default();
    let judged = decisions(&judged).map_err(io::Error::other)?;
    let pairs = match &plan {
        Some(plan) => pair_states(plan, &log).map_err(io::Error::other)?,
        None => Vec::new(),
    };
    let snapshot = Snapshot {
        name: env.name,
        up_seconds: SystemTime::now()
            .duration_since(up)
            .map_or(0, |age| age.as_secs()),
        mode: if plan.is_some() { "pair" } else { "plain" },
        authority: listening.trim().to_owned(),
        ledger: ledger.map_or(0, |ledger| {
            ledger
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count()
        }),
        pairs,
        refusal: last_refusal(&judged),
    };
    print!("{}", render(&snapshot));
    Ok(())
}
