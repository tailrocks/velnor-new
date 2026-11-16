// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::Ordering;
use std::time::Duration;

use bollard::errors::Error as DockerError;
use bollard::query_parameters::WaitContainerOptions;
use futures_util::StreamExt;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;
use velnor_resource_probe::job_hook::Challenge;

use crate::error::HostError;

use super::authority::Minted;
use super::channel::{
    REQUEST_POLL, challenge, deliver_jit, publish_challenge, publish_decision, read_request,
};
use super::decide::{Verdict, decide};
use super::pair::{LocalPair, remove_pair, remove_pair_named, start_pair};
use super::plan::Side;
use super::supervisor::{Slot, Supervisor, stopped};

#[derive(Serialize)]
struct Line<'a> {
    pair: &'a str,
    side: &'static str,
    agent: &'a str,
    allow: bool,
    reason: Option<&'static str>,
    ledger: Option<&'a str>,
    run_id: Option<u64>,
    attempt: Option<u32>,
    job: Option<&'a str>,
    event: Option<&'a str>,
    #[serde(rename = "ref")]
    git_ref: Option<&'a str>,
    sha: Option<&'a str>,
}

const PAIR_ATTEMPTS: u32 = 3;
const PAIR_RETRY: Duration = Duration::from_secs(2);

struct PairFailure {
    error: HostError,
    leftover: Option<String>,
    unretired: Option<String>,
}

impl From<HostError> for PairFailure {
    fn from(error: HostError) -> Self {
        Self {
            error,
            leftover: None,
            unretired: None,
        }
    }
}

struct Pending {
    error: HostError,
    leftover: Option<String>,
    unretired: Option<(Side, String)>,
}

async fn pause(stop: &mut watch::Receiver<bool>) {
    tokio::select! {
        () = tokio::time::sleep(PAIR_RETRY) => {}
        () = stopped(stop) => {}
    }
}

impl Supervisor {
    pub(super) async fn run_slot(
        &self,
        slot: Slot,
        mut stop: watch::Receiver<bool>,
    ) -> Result<(), HostError> {
        let mut failed = 0;
        let mut pending: Option<Pending> = None;
        while !*stop.borrow() {
            if let Some(mut left) = pending.take()
                && self.clean(&slot, &mut left).await.is_err()
            {
                failed += 1;
                if failed >= PAIR_ATTEMPTS {
                    return Err(left.error);
                }
                pending = Some(left);
                pause(&mut stop).await;
                continue;
            }
            match self.one_pair(&slot, &mut stop).await {
                Err(failure)
                    if (failure.error == HostError::Docker || failure.unretired.is_some())
                        && failed + 1 < PAIR_ATTEMPTS =>
                {
                    failed += 1;
                    eprintln!("pair {} failed at Docker, attempt {failed}", slot.label);
                    pending = Some(Pending {
                        error: failure.error,
                        leftover: failure.leftover,
                        unretired: failure.unretired.map(|name| (slot.side, name)),
                    });
                    pause(&mut stop).await;
                }
                Err(failure) => return Err(failure.error),
                Ok(()) => failed = 0,
            }
        }
        match pending {
            Some(mut left) => self.clean(&slot, &mut left).await.map_err(|()| left.error),
            None => Ok(()),
        }
    }

    async fn clean(&self, slot: &Slot, left: &mut Pending) -> Result<(), ()> {
        if let Some(name) = left.leftover.take() {
            if remove_pair_named(&self.docker, &name).await.is_err() {
                eprintln!("pair {} ({name}) was not removed", slot.label);
                left.leftover = Some(name);
                return Err(());
            }
            self.pairs_removed.fetch_add(1, Ordering::SeqCst);
            println!("pair {} removed", slot.label);
        }
        if let Some((side, name)) = left.unretired.take()
            && self.retire_agent(side, &name).await.is_err()
        {
            eprintln!("agent {name} was not retired at runner.server");
            left.unretired = Some((side, name));
            return Err(());
        }
        Ok(())
    }

    async fn one_pair(
        &self,
        slot: &Slot,
        stop: &mut watch::Receiver<bool>,
    ) -> Result<(), PairFailure> {
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let minted = self
            .client
            .mint(slot.side)
            .await
            .map_err(|_| PairFailure::from(HostError::Endpoint))?;
        let request = self.request(slot, &minted.name, sequence);
        let pair = match start_pair(&self.docker, &request).await {
            Ok(pair) => pair,
            Err(failure) => {
                if let Some(name) = &failure.leftover {
                    eprintln!("pair {} ({name}) was not cleaned", slot.label);
                }
                let retired = self.retire_agent(slot.side, &minted.name).await;
                return Err(PairFailure {
                    error: failure.error,
                    leftover: failure.leftover,
                    unretired: retired.is_err().then(|| minted.name.clone()),
                });
            }
        };
        let served = self.serve(slot, &pair, &minted, sequence, stop).await;
        let removed = remove_pair(&self.docker, &pair).await;
        let leftover = removed.is_err().then(|| pair.name().to_owned());
        if removed.is_ok() {
            self.pairs_removed.fetch_add(1, Ordering::SeqCst);
            println!("pair {} removed", slot.label);
        }
        let retired = self.retire_agent(slot.side, &minted.name).await;
        let unretired = retired.is_err().then(|| minted.name.clone());
        let error = served
            .err()
            .or_else(|| removed.err())
            .or_else(|| retired.err());
        error.map_or(Ok(()), |error| {
            Err(PairFailure {
                error,
                leftover,
                unretired,
            })
        })
    }

    async fn retire_agent(&self, side: Side, name: &str) -> Result<(), HostError> {
        self.client
            .delete_agent(side, name)
            .await
            .map_err(|_| HostError::Cleanup)?;
        self.agents_removed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn serve(
        &self,
        slot: &Slot,
        pair: &LocalPair,
        minted: &Minted,
        sequence: u32,
        stop: &mut watch::Receiver<bool>,
    ) -> Result<(), HostError> {
        let challenge = challenge(pair, sequence)?;
        publish_challenge(&self.docker, pair, &challenge).await?;
        deliver_jit(&self.docker, pair, &minted.jit).await?;
        println!("pair {} listening", slot.label);
        let mut exited = std::pin::pin!(wait_exit(&self.docker, pair.runner_id()));
        let request = tokio::select! {
            () = stopped(stop) => return Ok(()),
            outcome = &mut exited => return outcome,
            request = self.next_request(pair) => request?,
        };
        let verdict = decide(&self.client, slot.side, &challenge, &request).await;
        self.record(slot, &challenge, &verdict).await?;
        publish_decision(&self.docker, pair, &verdict.decision).await?;
        tokio::select! {
            () = stopped(stop) => Ok(()),
            outcome = &mut exited => outcome,
        }
    }

    async fn next_request(&self, pair: &LocalPair) -> Result<Vec<u8>, HostError> {
        loop {
            if let Some(bytes) = read_request(&self.docker, pair).await? {
                return Ok(bytes);
            }
            tokio::time::sleep(REQUEST_POLL).await;
        }
    }

    async fn record(
        &self,
        slot: &Slot,
        challenge: &Challenge,
        verdict: &Verdict,
    ) -> Result<(), HostError> {
        let facts = verdict.facts.as_ref();
        let line = Line {
            pair: &slot.label,
            side: slot.side.name(),
            agent: &challenge.runner_name,
            allow: verdict.decision.allow,
            reason: verdict.reason.map(super::decide::Reason::code),
            ledger: verdict.ledger.as_deref(),
            run_id: facts.map(|facts| facts.run_id),
            attempt: facts.map(|facts| facts.attempt),
            job: facts.map(|facts| facts.job.as_str()),
            event: facts.map(|facts| facts.event.as_str()),
            git_ref: facts.map(|facts| facts.git_ref.as_str()),
            sha: facts.map(|facts| facts.sha.as_str()),
        };
        let mut text = serde_json::to_string(&line).map_err(|_| HostError::Identity)?;
        text.push('\n');
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.decisions)
            .await
            .map_err(|_| HostError::Path)?;
        file.write_all(text.as_bytes())
            .await
            .map_err(|_| HostError::Path)?;
        file.flush().await.map_err(|_| HostError::Path)?;
        println!("decision {}", text.trim_end());
        Ok(())
    }
}

async fn wait_exit(docker: &bollard::Docker, id: &str) -> Result<(), HostError> {
    let mut waiting = docker.wait_container(id, None::<WaitContainerOptions>);
    match waiting.next().await {
        None
        | Some(
            Ok(_)
            | Err(
                DockerError::DockerContainerWaitError { .. }
                | DockerError::DockerResponseServerError {
                    status_code: 404, ..
                },
            ),
        ) => Ok(()),
        Some(Err(_)) => Err(HostError::Docker),
    }
}
