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

use super::authority::{CallError, Minted};
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

struct Unretired {
    name: String,
    deleted: bool,
}

struct PairFailure {
    error: HostError,
    leftover: Option<String>,
    unretired: Option<Box<Unretired>>,
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
    unretired: Option<(Side, Box<Unretired>)>,
}

fn unretired(pending: bool, name: &str, deleted: bool) -> PairFailure {
    PairFailure {
        error: HostError::Endpoint,
        leftover: None,
        unretired: pending.then(|| {
            Box::new(Unretired {
                name: name.to_owned(),
                deleted,
            })
        }),
    }
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
                    if (matches!(failure.error, HostError::Docker | HostError::Endpoint)
                        || failure.unretired.is_some())
                        && failed + 1 < PAIR_ATTEMPTS =>
                {
                    failed += 1;
                    eprintln!(
                        "pair {} failed: {}, attempt {failed}",
                        slot.label, failure.error
                    );
                    pending = Some(Pending {
                        error: failure.error,
                        leftover: failure.leftover,
                        unretired: failure.unretired.map(|left| (slot.side, left)),
                    });
                    pause(&mut stop).await;
                }
                Err(failure) => {
                    eprintln!("pair {} failed: {}", slot.label, failure.error);
                    return Err(failure.error);
                }
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
        if let Some((side, unretired)) = left.unretired.take() {
            let name = &unretired.name;
            let retired = if unretired.deleted {
                self.minted.retired(side.name(), name).await
            } else {
                self.retire_agent(side, name).await
            };
            if retired.is_err() {
                eprintln!("agent {name} was not retired");
                left.unretired = Some((side, unretired));
                return Err(());
            }
        }
        Ok(())
    }

    async fn one_pair(
        &self,
        slot: &Slot,
        stop: &mut watch::Receiver<bool>,
    ) -> Result<(), PairFailure> {
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let name = self.agent_name(sequence);
        self.minted.intent(slot.side.name(), &name).await?;
        let minted = tokio::select! {
            () = stopped(stop) => return Ok(()),
            minted = self.client.mint(slot.side, &name) => match minted {
                Ok(minted) if minted.name == name => minted,
                Ok(other) => {
                    self.minted.intent(slot.side.name(), &other.name).await?;
                    self.retire_agent(slot.side, &other.name).await?;
                    let abandoned = self.abandon(slot.side, &name).await;
                    return Err(unretired(abandoned.is_err(), &name, true));
                }
                Err(CallError::Refused(409) | CallError::CleanedUp) => {
                    let retired = self.minted.retired(slot.side.name(), &name).await;
                    return Err(unretired(retired.is_err(), &name, true));
                }
                Err(_) => {
                    let retired = self.abandon(slot.side, &name).await;
                    return Err(unretired(retired.is_err(), &name, true));
                }
            }
        };
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
                    unretired: retired.is_err().then(|| {
                        Box::new(Unretired {
                            name: minted.name.clone(),
                            deleted: false,
                        })
                    }),
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
        let unretired = retired.is_err().then(|| {
            Box::new(Unretired {
                name: minted.name.clone(),
                deleted: false,
            })
        });
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

    fn agent_name(&self, sequence: u32) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        format!("{}-jit-{nanos:x}-{sequence}", self.plan.environment)
    }

    async fn abandon(&self, side: Side, name: &str) -> Result<(), HostError> {
        if matches!(self.client.delete_agent(side, name).await, Ok(true)) {
            self.minted.retired(side.name(), name).await?;
        }
        Ok(())
    }

    async fn retire_agent(&self, side: Side, name: &str) -> Result<(), HostError> {
        self.client
            .delete_agent(side, name)
            .await
            .map_err(|_| HostError::Cleanup)?;
        self.minted.retired(side.name(), name).await?;
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
        let verdict = decide(
            &self.client,
            slot.side,
            self.policy.as_ref(),
            &challenge,
            &request,
        )
        .await;
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
        Some(Ok(response)) if response.status_code == 0 && response.error.is_none() => Ok(()),
        Some(Ok(_) | Err(DockerError::DockerContainerWaitError { .. })) => {
            Err(HostError::RunnerExited)
        }
        Some(Err(_)) | None => Err(HostError::Docker),
    }
}
