use std::future::Ready;

use super::super::Drive;
use super::super::bind::Bind;
use super::{Listed, classify_list};

const RUNNER_NAME: &str = "m100000788";
use crate::HostError;
use crate::journal::{IntentState, Journal, Outcome};
use crate::launch_harness::{Mode, Script, absent, assigned_wait, open};
use crate::worker::Started;

fn creds() -> Drive {
    Drive {
        set_id: 1,
        queue_path: "queues/messages".to_owned(),
        queue_token: "queue-token".to_owned(),
        admin_token: "admin-token".to_owned(),
        docker_engine_id: None,
        owner: "acme".to_owned(),
        repo: "widget".to_owned(),
        pat: "test-pat".to_owned(),
    }
}

fn start(volume: &str, _jit: &[u8], _bind: Bind) -> Ready<Result<Started, HostError>> {
    let ok = crate::launch_test_support::valid_worker_volume(volume);
    std::future::ready(if ok {
        Ok(Started {
            dind_id: "dind-1".to_owned(),
            runner_id: "runner-1".to_owned(),
        })
    } else {
        Err(HostError::ForbiddenMount)
    })
}

async fn seed_uncertain(journal: &Journal) -> Result<i64, String> {
    let id = journal
        .begin("launch", RUNNER_NAME)
        .await
        .map_err(|err| err.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|err| err.to_string())?;
    Ok(id)
}

async fn drive(
    mode: Mode,
    journal: &Journal,
) -> (Vec<&'static str>, Result<Option<Started>, String>) {
    let mut script = Script {
        calls: Vec::new(),
        mode,
    };
    let started = super::super::drive_offer(
        &mut script,
        &creds(),
        &assigned_wait(100_000_788, 4),
        journal,
        start,
    )
    .await
    .map_err(|err| err.to_string());
    (script.calls, started)
}

#[test]
fn offline_name_keeps_its_id() {
    let body = br#"{"total_count":1,"runners":[{"id":20231,"name":"m100000788","status":"offline","busy":false}]}"#;
    assert_eq!(classify_list(body, RUNNER_NAME), Listed::Offline(20231));
}

#[test]
fn busy_runner_is_live() {
    let body = br#"{"total_count":1,"runners":[{"id":20231,"name":"m100000788","status":"online","busy":true}]}"#;
    assert_eq!(classify_list(body, RUNNER_NAME), Listed::Live);
}

#[test]
fn a_short_page_is_not_absence() {
    let body =
        br#"{"total_count":2,"runners":[{"id":1,"name":"other","status":"offline","busy":false}]}"#;
    assert_eq!(classify_list(body, RUNNER_NAME), Listed::Unknown);
}

#[test]
fn a_full_page_without_the_name_is_absence() {
    let body = br#"{"total_count":0,"runners":[]}"#;
    assert_eq!(classify_list(body, RUNNER_NAME), Listed::Absent);
}

async fn slot_holds(journal: &Journal, id: i64) -> Result<bool, String> {
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    let row = rows
        .into_iter()
        .find(|row| row.id == id)
        .ok_or("missing row")?;
    Ok(super::super::slot::holds(&row))
}

fn one_done(rows: &[crate::reconcile::IntentRow]) -> bool {
    rows.iter()
        .any(|row| row.state == IntentState::Done && row.docker_id.as_deref() == Some("runner-1"))
}

#[tokio::test]
async fn empty_uncertain_offline_runner_is_deleted_and_reminted() -> Result<(), String> {
    let (scratch, journal) = open("empty-offline").await?;
    let stale = seed_uncertain(&journal).await?;
    let (calls, started) = drive(Mode::OfflineRunner, &journal).await;
    if started?.is_none() {
        return Err("mint did not start".to_owned());
    }
    assert_eq!(calls, ["runners-list", "runner-delete", "jit", "ack"]);
    if slot_holds(&journal, stale).await? {
        return Err("empty row still holds a slot".to_owned());
    }
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    let occupied = rows
        .iter()
        .filter(|row| super::super::slot::holds(row))
        .count();
    assert_eq!(occupied, 1);
    assert!(rows.iter().any(|row| {
        row.id == stale && row.state == IntentState::Failed && row.docker_id.is_none()
    }));
    assert!(one_done(&rows));
    let bytes = std::fs::read(scratch.file()).map_err(|err| err.to_string())?;
    if String::from_utf8_lossy(&bytes).contains("test-pat") {
        return Err("journal stored the rest credential".to_owned());
    }
    absent(&scratch.file())
}

#[tokio::test]
async fn busy_runner_is_not_deleted() -> Result<(), String> {
    let (scratch, journal) = open("busy-runner").await?;
    let stale = seed_uncertain(&journal).await?;
    let (calls, started) = drive(Mode::BusyRunner, &journal).await;
    assert_eq!(started, Err("effect uncertain".to_owned()));
    assert_eq!(calls, ["runners-list"]);
    if !slot_holds(&journal, stale).await? {
        return Err("busy name released the slot".to_owned());
    }
    let stored = journal.read(stale).await.map_err(|err| err.to_string())?;
    assert_eq!(stored, IntentState::Uncertain);
    absent(&scratch.file())
}

#[tokio::test]
async fn list_failure_keeps_the_uncertain_row() -> Result<(), String> {
    let (scratch, journal) = open("list-fail").await?;
    let stale = seed_uncertain(&journal).await?;
    let (calls, started) = drive(Mode::ListFail, &journal).await;
    assert_eq!(started, Err("effect uncertain".to_owned()));
    assert_eq!(calls, ["runners-list"]);
    if !slot_holds(&journal, stale).await? {
        return Err("list error released the slot".to_owned());
    }
    absent(&scratch.file())
}

#[tokio::test]
async fn delete_failure_keeps_the_uncertain_row() -> Result<(), String> {
    let (scratch, journal) = open("delete-fail").await?;
    let stale = seed_uncertain(&journal).await?;
    let (calls, started) = drive(Mode::DeleteFail, &journal).await;
    assert_eq!(started, Err("effect uncertain".to_owned()));
    assert_eq!(calls, ["runners-list", "runner-delete"]);
    if !slot_holds(&journal, stale).await? {
        return Err("delete error released the slot".to_owned());
    }
    absent(&scratch.file())
}

#[tokio::test]
async fn dind_row_is_not_cleared() -> Result<(), String> {
    let (scratch, journal) = open("dind-row").await?;
    let stale = seed_uncertain(&journal).await?;
    journal
        .bind_worker(stale, None, Some("dind-kept"))
        .await
        .map_err(|err| err.to_string())?;
    let (calls, started) = drive(Mode::OfflineRunner, &journal).await;
    assert_eq!(started, Err("effect uncertain".to_owned()));
    assert_eq!(calls, Vec::<&str>::new());
    if !slot_holds(&journal, stale).await? {
        return Err("dind row was cleared".to_owned());
    }
    absent(&scratch.file())
}

#[tokio::test]
async fn volume_row_is_not_cleared() -> Result<(), String> {
    let (scratch, journal) = open("volume-row").await?;
    let stale = seed_uncertain(&journal).await?;
    journal
        .bind_worker_volume(stale, "w9")
        .await
        .map_err(|err| err.to_string())?;
    let (calls, started) = drive(Mode::OfflineRunner, &journal).await;
    assert_eq!(started, Err("effect uncertain".to_owned()));
    assert_eq!(calls, Vec::<&str>::new());
    if !slot_holds(&journal, stale).await? {
        return Err("volume row was cleared".to_owned());
    }
    absent(&scratch.file())
}

#[tokio::test]
async fn empty_row_keeps_a_mint_retry_after_delete() -> Result<(), String> {
    let (scratch, journal) = open("empty-retry").await?;
    let stale = seed_uncertain(&journal).await?;
    let (calls, started) = drive(Mode::NameTakenOnce, &journal).await;
    if started?.is_none() {
        return Err("retry mint did not start".to_owned());
    }
    assert_eq!(
        calls,
        [
            "runners-list",
            "runner-delete",
            "jit",
            "runners-list",
            "jit",
            "ack"
        ]
    );
    if slot_holds(&journal, stale).await? {
        return Err("stale row holds a slot after retry".to_owned());
    }
    absent(&scratch.file())
}

#[tokio::test]
async fn exhausted_name_clear_does_not_ack() -> Result<(), String> {
    let (scratch, journal) = open("name-taken").await?;
    let (calls, started) = drive(Mode::NameTaken, &journal).await;
    assert_eq!(started, Err("runner name unchanged".to_owned()));
    assert_eq!(
        calls,
        [
            "jit",
            "runners-list",
            "runner-delete",
            "jit",
            "runners-list"
        ]
    );
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    let occupied = rows
        .iter()
        .filter(|row| super::super::slot::holds(row))
        .count();
    assert_eq!(occupied, 0);
    absent(&scratch.file())
}

#[tokio::test]
async fn same_offline_id_is_not_deleted_on_the_next_poll() -> Result<(), String> {
    let (scratch, journal) = open("name-again").await?;
    let (first, started) = drive(Mode::NameTaken, &journal).await;
    assert_eq!(started, Err("runner name unchanged".to_owned()));
    assert_eq!(
        first
            .iter()
            .filter(|call| **call == "runner-delete")
            .count(),
        1
    );
    let (second, again) = drive(Mode::NameTaken, &journal).await;
    assert_eq!(again, Err("runner name unchanged".to_owned()));
    assert_eq!(second, ["jit", "runners-list", "jit", "runners-list"]);
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    let occupied = rows
        .iter()
        .filter(|row| super::super::slot::holds(row))
        .count();
    assert_eq!(occupied, 0);
    absent(&scratch.file())
}

#[tokio::test]
async fn fresh_jit_conflict_deletes_the_offline_runner_and_starts() -> Result<(), String> {
    let (scratch, journal) = open("fresh-409").await?;
    let (calls, started) = drive(Mode::NameTakenOnce, &journal).await;
    if started?.is_none() {
        return Err("second mint did not start".to_owned());
    }
    assert_eq!(
        calls,
        ["jit", "runners-list", "runner-delete", "jit", "ack"]
    );
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    let occupied = rows
        .iter()
        .filter(|row| super::super::slot::holds(row))
        .count();
    assert_eq!(occupied, 1);
    assert!(
        rows.iter()
            .any(|row| row.state == IntentState::Failed && row.docker_id.is_none())
    );
    assert!(one_done(&rows));
    absent(&scratch.file())
}

#[tokio::test]
async fn job_subject_does_not_delete_the_same_offline_id_again() -> Result<(), String> {
    let (scratch, journal) = open("job-subject").await?;
    let (first, started) = drive_job(Mode::JobNameTaken, &journal).await;
    assert_eq!(started, Err("runner name cleared".to_owned()));
    assert_eq!(
        first
            .iter()
            .filter(|call| **call == "runner-delete")
            .count(),
        1
    );
    assert!(!first.contains(&"ack"));
    let (second, again) = drive_job(Mode::JobNameTaken, &journal).await;
    assert_eq!(again, Err("runner name cleared".to_owned()));
    assert_eq!(
        second
            .iter()
            .filter(|call| **call == "runner-delete")
            .count(),
        0
    );
    let rows = journal.rows().await.map_err(|err| err.to_string())?;
    assert!(rows.iter().any(|row| {
        row.subject == "m4r7"
            && row.state == IntentState::Failed
            && row.github_runner_id.as_deref() == Some("20246")
    }));
    let occupied = rows
        .iter()
        .filter(|row| super::super::slot::holds(row))
        .count();
    assert_eq!(occupied, 0);
    absent(&scratch.file())
}

async fn drive_job(
    mode: Mode,
    journal: &Journal,
) -> (Vec<&'static str>, Result<Option<Started>, String>) {
    let mut script = Script {
        calls: Vec::new(),
        mode,
    };
    let started = super::super::drive_offer(
        &mut script,
        &creds(),
        &crate::launch_harness::available(&[7]),
        journal,
        start,
    )
    .await
    .map_err(|err| err.to_string());
    (script.calls, started)
}
