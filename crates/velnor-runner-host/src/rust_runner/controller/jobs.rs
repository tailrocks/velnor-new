// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use tokio::sync::oneshot;
use velnor_runner_github::EncodedJit;

use super::admission::JobExit;
use super::cleanup::{CleanupProof, ProofInputs, prove};
use super::{Event, MACHINE_PROBE, Run, blocking};
use crate::launch_blocking::ListenFault;
use crate::rust_runner::{ReservationId, RunningJob};

async fn supervise(
    reservation: ReservationId,
    mut job: RunningJob,
    stop: oneshot::Receiver<()>,
) -> Event {
    let binary = job.binary().to_owned();
    let exit = tokio::select! {
        status = job.wait() => if let Ok(status) = status {
            JobExit::Exited(status)
        } else {
            let _ = job.stop().await;
            JobExit::Stopped
        },
        _ = stop => {
            let _ = job.stop().await;
            JobExit::Stopped
        }
    };
    Event::Ended {
        reservation,
        exit,
        binary,
    }
}

impl Run<'_> {
    pub(super) fn allocate_reservation(&mut self) -> Option<ReservationId> {
        let reservation = ReservationId::new(self.next_reservation)?;
        self.next_reservation = self.next_reservation.checked_add(1)?;
        Some(reservation)
    }

    pub(super) fn runner_name(&self, connection: usize, reservation: ReservationId) -> String {
        let scale_set = self
            .connections
            .get(connection)
            .map_or("", |live| live.binding.scale_set.name());
        format!("{scale_set}-{reservation}")
    }

    pub(super) fn launch(&mut self, reservation: ReservationId, jit: &EncodedJit) {
        let Some(facts) = self.slots.launching_facts(reservation) else {
            eprintln!("rust-runner has no trusted facts for reservation {reservation}");
            self.machine_ok = false;
            self.begin_proof(reservation, JobExit::SpawnFailed);
            return;
        };
        match self
            .cx
            .launcher
            .start_job(reservation, jit, &facts, &self.guest)
        {
            Ok(job) => {
                let (stop, stopped) = oneshot::channel();
                if self.slots.start_running(reservation, stop) {
                    eprintln!("rust_runner_job=started reservation={reservation}");
                    self.tasks.spawn(supervise(reservation, job, stopped));
                }
            }
            Err(error) => {
                eprintln!("rust-runner start failed for reservation {reservation}: {error}");
                self.machine_ok = false;
                self.begin_proof(reservation, JobExit::SpawnFailed);
            }
        }
    }

    pub(super) fn note_exit(&mut self, exit: &JobExit, binary: &str) {
        let JobExit::Exited(status) = exit else {
            return;
        };
        if let Some(failures) = self.runner.exited(status.success(), binary) {
            eprintln!(
                "rust-runner closing admission after {failures} consecutive failed runner exits until the runner binary changes or the daemon restarts"
            );
        }
    }

    pub(super) fn begin_proof(&mut self, reservation: ReservationId, exit: JobExit) {
        let described = exit.describe();
        if let Some(grant) = self.slots.begin_proof(reservation, exit) {
            eprintln!(
                "rust_runner_job=ended reservation={reservation} exit={described} {}",
                grant.describe()
            );
        }
        self.spawn_due_proofs();
    }

    pub(super) fn spawn_due_proofs(&mut self) {
        for (reservation, connection) in self.slots.claim_due_proofs() {
            let Some(live) = self.connections.get(connection) else {
                self.slots.proof_failed(reservation);
                continue;
            };
            let inputs = ProofInputs {
                launcher: self.cx.launcher.clone(),
                admin: live.admin.clone(),
                scale_set_id: live.scale_set_id,
                runner_name: self.runner_name(connection, reservation),
                reservation,
            };
            self.tasks.spawn(async move {
                let result = blocking(move || prove(&inputs)).await;
                Event::Proved {
                    reservation,
                    result,
                }
            });
        }
    }

    pub(super) fn on_proved(
        &mut self,
        reservation: ReservationId,
        result: Result<CleanupProof, ListenFault>,
    ) {
        match result {
            Ok(proof) => {
                if let Some(exit) = self.slots.release(proof) {
                    eprintln!(
                        "rust_runner_job=cleaned reservation={reservation} exit={}",
                        exit.describe()
                    );
                }
            }
            Err(error) => {
                eprintln!("rust-runner cleanup held for reservation {reservation}: {error}");
                self.slots.proof_failed(reservation);
            }
        }
    }

    pub(super) fn check_runner(&mut self) {
        if !self.runner.begin_check() {
            return;
        }
        let launcher = self.cx.launcher.clone();
        self.tasks.spawn(async move {
            let result = blocking(move || launcher.runner_digest().map_err(Into::into)).await;
            Event::RunnerBinary(result)
        });
    }

    pub(super) fn check_machine(&mut self) {
        if self.machine_checking
            || (self.machine_ok && self.machine_probed.elapsed() < MACHINE_PROBE)
        {
            return;
        }
        self.machine_checking = true;
        let launcher = self.cx.launcher.clone();
        self.tasks.spawn(async move {
            let result = blocking(move || launcher.ensure_machine().map_err(Into::into)).await;
            Event::Machine(result)
        });
    }
}
