// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command, time::Duration};

use velnor_runner_github::{
    AdminConnectionCall, admin_connection, create_session,
    workflow_run::{RepositoryTarget, get_repository_scope},
};

use super::{
    ConnectionPollContext, ControlState, Controller, FAILED, FairnessTurn, HostConfig, Restarted,
    TestResult, UNCERTAIN, assert_retired, freeze, poll_active_connections,
    restarted_with_a_removed_connection, seed, stalled,
};
use crate::host_controller::loop_driver::state::SessionCredentials;
use crate::https::HttpsTransport;
use crate::listen::{Secret, link_from_admin};

const CHILD: &str = "VELNOR_RETIREMENT_CURL_LOG";
const GATE: &str = "VELNOR_RETIREMENT_CURL_GATE";
const STALLED_BUDGET: Duration = Duration::from_millis(500);
const SLOW_POLL_BUDGET: Duration = Duration::from_secs(5);

#[test]
fn a_slow_removed_connection_retirement_does_not_hold_the_next_connection_poll() -> TestResult<()> {
    run(
        true,
        concat!(
            module_path!(),
            "::a_slow_removed_connection_retirement_does_not_hold_the_next_connection_poll"
        ),
    )
}

#[test]
fn a_previous_slow_poll_does_not_spend_the_next_connections_retirement_budget() -> TestResult<()> {
    run(
        false,
        concat!(
            module_path!(),
            "::a_previous_slow_poll_does_not_spend_the_next_connections_retirement_budget"
        ),
    )
}

fn run(stall: bool, name: &str) -> TestResult<()> {
    if std::env::var_os(CHILD).is_some() {
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(exercise(stall));
    }
    let dir = tempfile::tempdir()?;
    let mut command = match std::env::var("VELNOR_TOOL_BUN") {
        Ok(version) => {
            let mut command = Command::new("mise");
            command
                .args(["--no-config", "--no-env", "--no-hooks", "exec"])
                .arg(format!("bun@{version}"))
                .args(["--", "bun"])
                .env("MISE_AUTO_INSTALL", "false")
                .env("MISE_EXEC_AUTO_INSTALL", "false");
            command
        }
        Err(std::env::VarError::NotPresent) => Command::new("bun"),
        Err(error) => return Err(error.into()),
    };
    let bun = command.args(["--print", "process.execPath"]).output()?;
    assert!(
        bun.status.success(),
        "{}",
        String::from_utf8_lossy(&bun.stderr)
    );
    let executable = String::from_utf8(bun.stdout)?;
    let script = format!("#!{}\n{}", executable.trim(), include_str!("curl.ts"));
    let script_path = dir.path().join("curl.ts");
    fs::write(&script_path, script)?;
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))?;
    std::os::unix::fs::symlink(&script_path, dir.path().join("curl"))?;
    let (_, name) = name.split_once("::").ok_or("test module")?;
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", name, "--nocapture"])
        .env("PATH", dir.path())
        .env(CHILD, dir.path().join("requests"))
        .envs((!stall).then(|| (GATE, dir.path().join("gate"))))
        .output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    Ok(())
}

async fn active_connection(restarted: &Restarted) -> TestResult<Controller> {
    let config =
        HostConfig::parse(&super::super::HOST_CONFIG.replace("conn-test-1", "conn-other-1"))?;
    let binding = config.connections().first().ok_or("active binding")?;
    let scope = restarted.journal.connection_scope(binding).await?;
    let connection = restarted.journal.connection().await?;
    connection.execute(
        "INSERT INTO connection_runtime_bindings SELECT 'conn-other-1', scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform FROM connection_runtime_bindings WHERE connection_id='conn-test-1'",
        (),
    ).await?;
    let mut active = Controller::with_config(&scope, &restarted.engine, config)?;
    let mut transport = HttpsTransport::new("https://api.github.com")?;
    let repository = get_repository_scope(
        &mut transport,
        RepositoryTarget {
            id: 42,
            owner: "owner",
            name: "repo",
        },
        "pat",
    )?;
    let admin = admin_connection(
        &mut transport,
        &AdminConnectionCall {
            config_url: "https://github.com/owner/repo",
            registration_token: "registration",
        },
    )?;
    let mut link = link_from_admin(transport, admin, &active.prepared.binding, &repository)?;
    let session = create_session(link.transport(), 7, "velnor-host", "admin-token")?;
    let nonce = "b".repeat(32);
    scope.prepare_session_create(&nonce).await?;
    let claim = scope
        .claim_session_create(&nonce)
        .await?
        .ok_or("session claim")?;
    let bound = scope
        .record_created_session(&claim, &session.session_id, 0)
        .await?;
    active.prepared.session_intent = bound.intent_id();
    active.prepared.credentials = Some(SessionCredentials {
        link,
        session,
        github_pat: Secret::new("pat"),
    });
    Ok(active)
}

/// While the next connection's poll waits in curl, moves the frozen clock past the retirement
/// budget and releases curl. The poll then took longer than the budget on the clock the deadlines
/// read.
fn slow_poll_in_curl(gate: String) {
    let runtime = tokio::runtime::Handle::current();
    std::thread::spawn(move || {
        while !PathBuf::from(format!("{gate}.waiting")).exists() {
            std::thread::sleep(Duration::from_millis(5));
        }
        runtime.block_on(tokio::time::advance(
            SLOW_POLL_BUDGET + Duration::from_secs(1),
        ));
        drop(fs::write(format!("{gate}.released"), ""));
    });
}

async fn exercise(stall: bool) -> TestResult<()> {
    let (restarted, removed) = restarted_with_a_removed_connection().await?;
    seed::assigned_pair(&restarted.journal, &removed.prepared.scope, FAILED).await?;
    crate::host_controller::loop_driver::setup::configure_shared_capacity(
        &restarted.journal,
        &removed.config,
    )
    .await?;
    let active = active_connection(&restarted).await?;
    let failed_objects = restarted.fixture.objects(FAILED);
    if stall {
        restarted
            .fixture
            .gate_container_inspection(&FAILED.container_id("runner"));
    } else {
        slow_poll_in_curl(std::env::var(GATE)?);
    }
    let control_state = ControlState::new(&removed.config);
    let pending = [removed.prepared.binding.clone()];
    let mut connections = if stall {
        vec![removed.prepared, active.prepared]
    } else {
        vec![active.prepared, removed.prepared]
    };
    let clock = freeze();
    {
        let poll = poll_active_connections(
            ConnectionPollContext {
                config: &removed.config,
                journal: &restarted.journal,
                docker: &restarted.docker,
                selected_engine: &restarted.engine,
                worker_images: &removed.images,
                state: removed.state.path(),
                image_authority: &removed.image_authority,
                admission_ready: false,
                control_state: &control_state,
                retirement_budget: if stall {
                    STALLED_BUDGET
                } else {
                    SLOW_POLL_BUDGET
                },
            },
            &pending,
            &mut connections,
            FairnessTurn::new(false, false),
        );
        tokio::pin!(poll);
        if stall {
            stalled(&restarted.fixture, &mut poll).await?;
            assert_eq!(restarted.fixture.inspection_gate_hits(), 1);
            clock
                .advance(STALLED_BUDGET + Duration::from_millis(500))
                .await;
        }
        poll.await?;
    }
    drop(clock);
    assert_recovery_outcome(stall, &restarted, failed_objects).await
}

async fn assert_recovery_outcome(
    stall: bool,
    restarted: &Restarted,
    failed_objects: Vec<String>,
) -> TestResult<()> {
    assert_polled(restarted).await?;
    if stall {
        assert_eq!(
            seed::text(&restarted.journal, seed::RESERVATION, FAILED.reservation).await?,
            UNCERTAIN
        );
        assert_ne!(failed_objects, Vec::<String>::new());
        assert_eq!(restarted.fixture.objects(FAILED), failed_objects);
    } else {
        assert!(PathBuf::from(format!("{}.released", std::env::var(GATE)?)).exists());
        assert_retired(restarted).await?;
    }
    Ok(())
}

async fn assert_polled(restarted: &Restarted) -> TestResult<()> {
    let log = fs::read_to_string(std::env::var(CHILD)?)?;
    assert!(log.contains("https://queue.example.test/messages"));
    let connection = restarted.journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT snapshot_json FROM capacity_header_meta WHERE singleton=1",
            (),
        )
        .await?;
    let snapshot: serde_json::Value = serde_json::from_str(
        &rows
            .next()
            .await?
            .ok_or("header snapshot")?
            .get::<String>(0)?,
    )?;
    let record = snapshot["records"]
        .as_array()
        .ok_or("header records")?
        .iter()
        .find(|record| record["identity"]["connection_id"] == "conn-other-1")
        .ok_or("active header")?;
    assert_eq!(record["attempt_sequence"], 1);
    assert_eq!(record["zero_header_settled"], true);
    assert!(record["in_flight_poll"].is_null());
    Ok(())
}
