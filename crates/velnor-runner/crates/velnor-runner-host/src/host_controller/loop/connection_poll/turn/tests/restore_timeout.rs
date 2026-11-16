// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command, time::Duration};

use velnor_runner_github::{
    AdminConnectionCall, admin_connection,
    workflow_run::{RepositoryTarget, get_repository_scope},
};

use super::{TestResult, fixture::Fixture};
use crate::{
    host_controller::loop_driver::connection_poll::credentials::{
        RestoreMode, restore_credentials_from_authority,
    },
    https::HttpsTransport,
    journal::Journal,
    listen::{Link, Secret, link_from_admin},
};

const TEST: &str = "host_controller::loop_driver::connection_poll::turn::tests::restore_timeout::a_timed_out_restore_releases_its_claim_before_retry";
const CHILD: &str = "VELNOR_RESTORE_TIMEOUT_FIXTURE";

#[tokio::test]
async fn a_timed_out_restore_releases_its_claim_before_retry() -> TestResult<()> {
    if std::env::var_os(CHILD).is_some() {
        return timed_out_restore().await;
    }
    let temp = tempfile::tempdir()?;
    let curl = temp.path().join("curl");
    let bun = Command::new("bun")
        .args(["-p", "process.execPath"])
        .output()?;
    assert!(bun.status.success());
    let bun = String::from_utf8(bun.stdout)?;
    let (_, script) = include_str!("restore_curl.ts")
        .split_once('\n')
        .ok_or("script")?;
    fs::write(&curl, format!("#!{}\n{script}", bun.trim()))?;
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o700))?;
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", TEST, "--nocapture"])
        .env(CHILD, temp.path())
        .env(
            "PATH",
            std::env::join_paths(std::iter::once(temp.path().to_path_buf()).chain(
                std::env::split_paths(&std::env::var_os("PATH").ok_or("PATH")?),
            ))?,
        )
        .output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    Ok(())
}

async fn timed_out_restore() -> TestResult<()> {
    let mut fixture = Fixture::new().await?;
    fixture.bind_session().await?;
    let root = PathBuf::from(std::env::var_os(CHILD).ok_or("fixture directory")?);
    let journal = fixture.journal.clone();
    let writer = tokio::task::spawn_blocking(move || hold_terminal_write(&journal, &root));
    let began = std::time::Instant::now();
    let first_result = restore(&mut fixture, Duration::from_secs(10)).await;
    let completed_at = tokio::time::Instant::now().into_std();
    writer.await??;
    assert!(!first_result?);
    assert!(began.elapsed() >= Duration::from_secs(10));
    assert!(began.elapsed() < Duration::from_secs(60));
    assert!(fixture.prepared.credentials.is_none());
    let first = refreshes(&fixture).await?;
    let retry_at = fixture.prepared.restore_retry_at.ok_or("retry deadline")?;
    assert!(retry_at >= completed_at + Duration::from_secs(29));
    assert!(retry_at <= completed_at + Duration::from_secs(30));
    assert!(fixture.poll().await?.is_none());
    assert_eq!(refreshes(&fixture).await?, first);
    assert!(fixture.prepared.credentials.is_none());
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(30)).await;
    let recovered = restore(&mut fixture, Duration::from_secs(10)).await?;
    let after = refreshes(&fixture).await?;
    eprintln!(
        "restore evidence first={first:?} after={after:?} recovered={recovered} credentials={}",
        fixture.prepared.credentials.is_some()
    );
    assert_eq!(first, [(1, "transport_error".to_owned())]);
    assert!(
        recovered,
        "a later restore must obtain credentials without restart"
    );
    assert!(fixture.prepared.credentials.is_some());
    assert!(fixture.prepared.restore_retry_at.is_none());
    assert_eq!(
        after,
        [
            (1, "transport_error".to_owned()),
            (2, "succeeded".to_owned())
        ]
    );
    std::thread::sleep(Duration::from_millis(10_100));
    assert!(fixture.poll().await?.is_some());
    assert!(fixture.control.complete_session_poll_receipts()?.is_some());
    Ok(())
}

fn hold_terminal_write(journal: &Journal, root: &std::path::Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    while !root.join("refresh-started").exists() {
        if started.elapsed() >= Duration::from_secs(30) {
            return Err("refresh transport did not start".to_owned());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let guard = runtime.block_on(journal.write_guard());
    fs::write(root.join("journal-held"), "held").map_err(|error| error.to_string())?;
    std::thread::sleep(Duration::from_secs(12));
    drop(guard);
    Ok(())
}

async fn restore(fixture: &mut Fixture, budget: Duration) -> TestResult<bool> {
    let link = authority(fixture)?;
    Ok(restore_credentials_from_authority(
        &mut fixture.prepared,
        RestoreMode::Polling,
        tokio::time::Instant::now() + budget,
        Ok((link, Secret::new("pat-test"), 7)),
    )
    .await)
}

fn authority(fixture: &Fixture) -> TestResult<Link> {
    let mut transport = HttpsTransport::new("https://actions.test")?;
    let scope = get_repository_scope(
        &mut transport,
        RepositoryTarget {
            id: 42,
            owner: "owner",
            name: "repo",
        },
        "pat-test",
    )?;
    let admin = admin_connection(
        &mut transport,
        &AdminConnectionCall {
            config_url: "https://github.com/owner/repo",
            registration_token: "registration-test",
        },
    )?;
    Ok(link_from_admin(
        transport,
        admin,
        &fixture.prepared.binding,
        &scope,
    )?)
}

async fn refreshes(fixture: &Fixture) -> TestResult<Vec<(i64, String)>> {
    let mut rows = fixture
        .journal
        .connection()
        .await?
        .query(
            "SELECT generation, state FROM connection_session_refreshes ORDER BY generation",
            (),
        )
        .await?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().await? {
        records.push((row.get(0)?, row.get(1)?));
    }
    Ok(records)
}
