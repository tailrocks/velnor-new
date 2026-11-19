// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::Value;
use velnor_runner_host::{CompletionWorker, connect_unix};

use super::super::super::TestResult;
use super::super::super::runtime_and_policy::sample_connection;
use super::helper_seed;
use super::helper_stub::VolumeState;
pub(crate) use super::helper_stub::{HelperSpec, Kind};
use super::{WorkflowEvent, seed, spawn_docker_stub};

pub(crate) struct CancelledObservation {
    pub(crate) helpers_left: usize,
    pub(crate) unmounted_left: usize,
    pub(crate) stops: usize,
    pub(crate) volumes_left: usize,
    pub(crate) present_left: usize,
    pub(crate) network_left: usize,
    pub(crate) network: String,
    pub(crate) phase: String,
    pub(crate) lease: String,
    pub(crate) initializer: String,
    pub(crate) reservation: String,
}

impl CancelledObservation {
    pub(crate) fn encode(&self) -> String {
        format!(
            "OBS {} {} {} {} {} {} {} {} {} {} {}",
            self.helpers_left,
            self.unmounted_left,
            self.stops,
            self.volumes_left,
            self.present_left,
            self.network_left,
            self.phase,
            self.lease,
            self.initializer,
            self.reservation,
            self.network
        )
    }

    pub(crate) fn decode(stdout: &str) -> Option<Self> {
        let line = stdout
            .lines()
            .find_map(|line| line.split_once("OBS "))
            .map(|(_, rest)| rest)?;
        let mut fields = line.split(' ');
        let mut count = || fields.next()?.parse::<usize>().ok();
        let (helpers_left, unmounted_left, stops, volumes_left, present_left, network_left) =
            (count()?, count()?, count()?, count()?, count()?, count()?);
        Some(Self {
            helpers_left,
            unmounted_left,
            stops,
            volumes_left,
            present_left,
            network_left,
            phase: fields.next()?.to_owned(),
            lease: fields.next()?.to_owned(),
            initializer: fields.next()?.to_owned(),
            reservation: fields.next()?.to_owned(),
            network: fields.next()?.to_owned(),
        })
    }
}

async fn query_first(conn: &turso::Connection, sql: &str) -> TestResult<String> {
    let mut rows = conn.query(sql, ()).await?;
    Ok(match rows.next().await? {
        Some(row) => row.get::<String>(0)?,
        None => "-".to_owned(),
    })
}

async fn journal_state(journal_path: &Path) -> TestResult<[String; 5]> {
    let db = turso::Builder::new_local(journal_path.to_str().ok_or("journal path utf8")?)
        .build()
        .await?;
    let conn = db.connect()?;
    Ok([
        query_first(&conn, "SELECT phase FROM resource_probe_operations WHERE helper_role = 'execution-publication'").await?,
        query_first(&conn, "SELECT state FROM host_operation_leases WHERE singleton = 1").await?,
        query_first(&conn, "SELECT state FROM capacity_reservations WHERE id = 2").await?,
        query_first(&conn, "SELECT remove_state || '/' || abort_state FROM capacity_reservation_storage_initialization WHERE reservation_id = 2").await?,
        query_first(&conn, "SELECT create_state || '/' || remove_state FROM capacity_reservation_networks WHERE reservation_id = 2").await?,
    ])
}

fn scenario_done(stub: &super::LocalDockerStub, expect_success: bool) -> TestResult<bool> {
    let guard = stub.state.lock().map_err(|_| "stub state poisoned")?;
    Ok(if expect_success {
        guard.volumes.all_released()
    } else {
        guard.volumes.listed() > 0
    })
}

pub(crate) fn run_cancelled_before_start_scenario(
    specs: &[HelperSpec],
    expect_success: bool,
    storage_proven: bool,
) -> TestResult<CancelledObservation> {
    let temp = tempfile::tempdir()?;
    let state_dir = temp.path().canonicalize()?;
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700))?;
    let socket_path = state_dir.join("docker.sock");
    let journal_path = state_dir.join("journal.db");
    let stub = spawn_docker_stub(&socket_path, 3, Vec::new())?;
    stub.release_dind_stop.store(true, Ordering::Release);
    let connection = sample_connection(vec![WorkflowEvent::Push], Vec::new())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let [phase, lease, reservation, initializer, network] = runtime.block_on(async {
        let journal = seed::seed_completed_launch_with(&journal_path, &connection, 2).await?;
        let instance_id = helper_seed::seed_helper_rows(
            &journal_path,
            specs.iter().any(|spec| spec.kind == Kind::Publication),
            specs.iter().any(|spec| spec.kind == Kind::Initializer),
            specs.iter().any(|spec| {
                spec.kind == Kind::Publication && spec.life == super::helper_stub::Life::Gone
            }),
        )
        .await?;
        if storage_proven {
            let db = turso::Builder::new_local(journal_path.to_str().ok_or("journal path utf8")?)
                .build()
                .await?;
            let conn = db.connect()?;
            let mut guards = Vec::new();
            let mut rows = conn
                .query(
                    "SELECT name FROM sqlite_schema WHERE type = 'trigger' AND sql LIKE '%NEW.storage_cleanup_proven = 1%'",
                    (),
                )
                .await?;
            while let Some(row) = rows.next().await? {
                guards.push(row.get::<String>(0)?);
            }
            drop(rows);
            for guard in guards {
                conn.execute(&format!("DROP TRIGGER {guard}"), ()).await?;
            }
            conn.execute(
                "UPDATE capacity_reservation_provisioning_state SET storage_cleanup_proven = 1 WHERE reservation_id = 2",
                (),
            )
            .await?;
        }
        stub.state
            .lock()
            .map_err(|_| "stub state poisoned")?
            .volumes = VolumeState::cancelled_before_start(specs, &instance_id);
        let docker = connect_unix(socket_path.to_str().ok_or("socket utf8")?)?;
        let worker = CompletionWorker::start_for_connection(
            &journal,
            &connection,
            docker,
            "https://api.github.com",
            "ghs_test_admin_token".to_owned(),
            "ghp_test_release_read_token",
            &state_dir,
        )
        .await?;
        let mut observer = None;
        let mut reached = false;
        for _ in 0..3000 {
            let done = if storage_proven {
                journal_state(&journal_path).await?[4] == "created/removed"
            } else {
                scenario_done(&stub, expect_success)?
                && if expect_success {
                    matches!(
                        journal_state(&journal_path).await?[3].as_str(),
                        "-" | "removed/not_requested"
                    )
                } else {
                    seed::retry_committed(&state_dir, &mut observer).await?
                }
            };
            if done {
                reached = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if !reached {
            let _outcome = worker.shutdown().await;
            return Err("cancelled-run scenario did not reach its terminal condition".into());
        }
        if !expect_success {
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        let _outcome = worker.shutdown().await;
        journal_state(&journal_path).await
    })?;
    let guard = stub.state.lock().map_err(|_| "stub state poisoned")?;
    let (helpers_left, unmounted_left, stops, volumes_left) = guard.volumes.counts();
    let present_left = guard.volumes.present_count();
    let network_left = guard.volumes.network_left();
    Ok(CancelledObservation {
        helpers_left,
        unmounted_left,
        stops,
        volumes_left,
        present_left,
        network_left,
        phase,
        lease,
        initializer,
        reservation,
        network,
    })
}

pub(super) fn read_diagnostics_files(
    state_dir: &Path,
) -> TestResult<(Option<Value>, Option<Value>)> {
    fn read_opt_json(path: &Path) -> TestResult<Option<Value>> {
        path.exists()
            .then(|| Ok(serde_json::from_slice(&std::fs::read(path)?)?))
            .transpose()
    }
    for entry in std::fs::read_dir(state_dir.join("completion-diagnostics"))
        .into_iter()
        .flatten()
    {
        let path = entry?.path();
        if path.is_dir() {
            return Ok((
                read_opt_json(&path.join("i1-r2-runner-log-state.json"))?,
                read_opt_json(&path.join("i1-g1-runner-logs.json"))?,
            ));
        }
    }
    Ok((None, None))
}
