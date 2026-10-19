use std::thread::{self, JoinHandle};

use crate::{config::HostConfig, error::HostError, journal::Journal};

use super::ControllerLoop;

pub(super) struct DoctorProbeWorker(Option<JoinHandle<()>>);

impl DoctorProbeWorker {
    pub(super) fn is_finished(&self) -> bool {
        self.0.as_ref().is_some_and(JoinHandle::is_finished)
    }
}

impl Drop for DoctorProbeWorker {
    fn drop(&mut self) {
        if let Some(worker) = self.0.take() {
            drop(worker.join());
        }
    }
}

impl ControllerLoop<'_> {
    pub(super) async fn start_doctor_probe(&mut self) {
        if self
            .doctor_worker
            .as_ref()
            .is_some_and(DoctorProbeWorker::is_finished)
        {
            drop(self.doctor_worker.take());
        }
        if self.doctor_worker.is_some() {
            return;
        }
        let request = match self.journal.active_doctor_probe_request().await {
            Ok(Some(request))
                if request.state().is_active()
                    && self.doctor_attempted_request_id != Some(request.request_id()) =>
            {
                request
            }
            Ok(_) => return,
            Err(error) => {
                eprintln!("doctor probe queue read held: {error}");
                return;
            }
        };
        let ticket = request.ticket();
        let failure_ticket = request.ticket();
        self.doctor_attempted_request_id = Some(request.request_id());
        let worker_config = self.current.clone();
        let worker_docker = self.docker.clone();
        let worker_journal = self.journal.clone();
        let worker_state = self.control_state.clone();
        let spawned = thread::Builder::new()
            .name("velnor-doctor-probe".to_owned())
            .spawn(move || {
                run_doctor_probe_worker(
                    worker_config,
                    worker_docker,
                    worker_journal,
                    worker_state,
                    ticket,
                );
            });
        self.doctor_worker = match spawned {
            Ok(worker) => Some(DoctorProbeWorker(Some(worker))),
            Err(error) => {
                eprintln!("doctor probe worker start failed: {error}");
                let _ = self
                    .journal
                    .fail_queued_doctor_probe_request(&failure_ticket)
                    .await;
                None
            }
        };
    }
}

fn run_doctor_probe_worker(
    config: HostConfig,
    docker: bollard::Docker,
    journal: Journal,
    state: crate::controller_ipc::ControlState,
    ticket: crate::journal::DoctorProbeTicket,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let result = match runtime {
        Ok(runtime) => match crate::launch::ReleaseReadSecret::load(&config) {
            Ok(secret) => runtime.block_on(crate::launch::run_doctor_probe(
                &docker, &journal, &secret, &ticket,
            )),
            Err(error) => {
                let _ = runtime.block_on(journal.fail_queued_doctor_probe_request(&ticket));
                Err(error)
            }
        },
        Err(_) => Err(HostError::Journal),
    };
    match result {
        Ok(Some(report)) => {
            let _ = state.save_doctor_report(ticket.nonce(), report);
        }
        Ok(None) => {}
        Err(error) => eprintln!("doctor probe worker failed: {error}"),
    }
}
