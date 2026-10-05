use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Answer, Facts, JournalFact, classify, controller_readiness, journal_mark};
use crate::journal::{Journal, Outcome};
use crate::readiness::Readiness;

const SERVICE: &str = "com.tailrocks.velnor.readiness-test";
const ACCOUNT: &str = "absent";

fn facts(
    drain: Answer,
    directory: Answer,
    config_ok: Answer,
    credential: Answer,
    engine: Answer,
    journal: JournalFact,
) -> Facts {
    Facts {
        drain,
        directory,
        config_ok,
        credential,
        engine,
        journal,
    }
}

#[test]
fn classify_matches_the_precedence() {
    let clear = JournalFact::Clear;
    assert_eq!(
        classify(facts(
            Answer::Yes,
            Answer::No,
            Answer::No,
            Answer::No,
            Answer::No,
            clear
        )),
        Readiness::Draining
    );
    assert_eq!(
        classify(facts(
            Answer::No,
            Answer::No,
            Answer::Yes,
            Answer::Yes,
            Answer::Yes,
            clear
        )),
        Readiness::WaitingForCredentials
    );
    assert_eq!(
        classify(facts(
            Answer::No,
            Answer::Yes,
            Answer::No,
            Answer::Yes,
            Answer::Yes,
            clear
        )),
        Readiness::WaitingForCredentials
    );
    assert_eq!(
        classify(facts(
            Answer::No,
            Answer::Yes,
            Answer::Yes,
            Answer::No,
            Answer::Yes,
            JournalFact::Occupied,
        )),
        Readiness::WaitingForCredentials
    );
}

#[test]
fn classify_reports_engine_journal_and_ready() {
    let clear = JournalFact::Clear;
    assert_eq!(
        classify(facts(
            Answer::No,
            Answer::Yes,
            Answer::Yes,
            Answer::Yes,
            Answer::No,
            JournalFact::Occupied,
        )),
        Readiness::WaitingForEngine
    );
    assert_eq!(
        classify(facts(
            Answer::No,
            Answer::Yes,
            Answer::Yes,
            Answer::Yes,
            Answer::Yes,
            JournalFact::Unreadable,
        )),
        Readiness::Degraded
    );
    assert_eq!(
        classify(facts(
            Answer::No,
            Answer::Yes,
            Answer::Yes,
            Answer::Yes,
            Answer::Yes,
            JournalFact::Occupied,
        )),
        Readiness::Reconciling
    );
    assert_eq!(
        classify(facts(
            Answer::No,
            Answer::Yes,
            Answer::Yes,
            Answer::Yes,
            Answer::Yes,
            clear,
        )),
        Readiness::Ready
    );
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-ready-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|err| err.to_string())?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let removed = std::fs::remove_dir_all(&self.path);
        let _kept = removed.err().map(|err| err.kind());
    }
}

fn sample() -> String {
    "schema = 1\n[github]\nrepository = \"example/repo\"\nscale_set_name = \"ubuntu-26.04-scale-set\"\ncredential_ref = \"keychain:test/absent\"\n[host]\nmax_jobs = 1\n[docker]\ncontext = \"test\"\nplatform = \"linux/amd64\"\nendpoint = \"unix:///tmp/velnor-readiness-absent.sock\"\n".to_owned()
}

#[test]
fn sync_entry_leaves_a_missing_directory_absent() -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("velnor-ready-sync-{}", std::process::id()));
    let _removed = std::fs::remove_dir_all(&path);
    let state = super::controller_readiness(&path, SERVICE, ACCOUNT);
    if path.exists() {
        return Err("created".to_owned());
    }
    if state != Readiness::WaitingForCredentials {
        return Err(state.as_str().to_owned());
    }
    Ok(())
}

#[test]
fn drain_file_reports_draining_without_config() -> Result<(), String> {
    let scratch = Scratch::new("drain")?;
    std::fs::write(scratch.path().join("drain"), b"1").map_err(|err| err.to_string())?;
    let state = controller_readiness(scratch.path(), SERVICE, ACCOUNT);
    if state == Readiness::Draining {
        Ok(())
    } else {
        Err(state.as_str().to_owned())
    }
}

#[test]
fn missing_keychain_item_beats_a_dead_socket() -> Result<(), String> {
    let scratch = Scratch::new("cred")?;
    std::fs::write(scratch.path().join("host.toml"), sample()).map_err(|err| err.to_string())?;
    let state = controller_readiness(scratch.path(), SERVICE, ACCOUNT);
    if state == Readiness::WaitingForCredentials {
        Ok(())
    } else {
        Err(state.as_str().to_owned())
    }
}

#[tokio::test]
async fn missing_journal_stays_clear_and_uncreated() -> Result<(), String> {
    let scratch = Scratch::new("absent")?;
    let path = scratch.path().join("launch.db");
    let mark = journal_mark(&path).await;
    if path.exists() {
        return Err("created".to_owned());
    }
    if mark == JournalFact::Clear {
        Ok(())
    } else {
        Err("mark".to_owned())
    }
}

#[tokio::test]
async fn garbage_journal_is_unreadable() -> Result<(), String> {
    let scratch = Scratch::new("garbage")?;
    let path = scratch.path().join("launch.db");
    std::fs::write(&path, b"not a database").map_err(|err| err.to_string())?;
    if journal_mark(&path).await == JournalFact::Unreadable {
        Ok(())
    } else {
        Err("mark".to_owned())
    }
}

#[tokio::test]
async fn pending_launch_occupies_and_failed_acquire_does_not() -> Result<(), String> {
    let scratch = Scratch::new("rows")?;
    let launch = scratch.path().join("launch.db");
    let journal = Journal::open(&launch).await.map_err(|_| "open")?;
    journal.begin("launch", "job").await.map_err(|_| "begin")?;
    if journal_mark(&launch).await != JournalFact::Occupied {
        return Err("launch".to_owned());
    }
    let acquire_path = scratch.path().join("acquire.db");
    let acquire = Journal::open(&acquire_path).await.map_err(|_| "open2")?;
    let id = acquire
        .begin("acquire", "job")
        .await
        .map_err(|_| "begin2")?;
    acquire
        .finish(id, Outcome::DefiniteFailure)
        .await
        .map_err(|_| "finish")?;
    if journal_mark(&acquire_path).await == JournalFact::Clear {
        Ok(())
    } else {
        Err("acquire".to_owned())
    }
}

#[tokio::test]
async fn proven_cleanup_clears_and_done_launch_stays() -> Result<(), String> {
    let scratch = Scratch::new("clean")?;
    let proven = scratch.path().join("proven.db");
    let journal = Journal::open(&proven).await.map_err(|_| "open")?;
    let id = journal.begin("launch", "job").await.map_err(|_| "begin")?;
    journal.record_cleanup(id).await.map_err(|_| "cleanup")?;
    if journal_mark(&proven).await != JournalFact::Clear {
        return Err("proven".to_owned());
    }
    let done = scratch.path().join("done.db");
    let finished = Journal::open(&done).await.map_err(|_| "open2")?;
    let row = finished
        .begin("launch", "other")
        .await
        .map_err(|_| "begin2")?;
    finished
        .finish(row, Outcome::Done)
        .await
        .map_err(|_| "done")?;
    if journal_mark(&done).await == JournalFact::Occupied {
        Ok(())
    } else {
        Err("done".to_owned())
    }
}

#[tokio::test]
async fn unknown_state_is_unreadable() -> Result<(), String> {
    let scratch = Scratch::new("bad")?;
    let path = scratch.path().join("launch.db");
    let journal = Journal::open(&path).await.map_err(|_| "open")?;
    journal.begin("launch", "job").await.map_err(|_| "begin")?;
    drop(journal);
    let text = path.to_str().ok_or("utf8")?;
    let db = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|_| "build")?;
    let conn = db.connect().map_err(|_| "connect")?;
    conn.execute("UPDATE intents SET state = ?1", ("bogus".to_owned(),))
        .await
        .map_err(|_| "update")?;
    drop(conn);
    if journal_mark(&path).await == JournalFact::Unreadable {
        Ok(())
    } else {
        Err("state".to_owned())
    }
}

#[test]
fn sync_drain_is_draining_without_a_probe() -> Result<(), String> {
    let scratch = Scratch::new("sync-drain")?;
    std::fs::write(scratch.path().join("drain"), b"1").map_err(|err| err.to_string())?;
    let state = super::controller_readiness(scratch.path(), SERVICE, ACCOUNT);
    if state == Readiness::Draining {
        Ok(())
    } else {
        Err(state.as_str().to_owned())
    }
}

#[tokio::test]
async fn unreadable_journal_paths_stay_unreadable() -> Result<(), String> {
    let scratch = Scratch::new("stat")?;
    let dir = scratch.path().join("dir.db");
    std::fs::create_dir(&dir).map_err(|err| err.to_string())?;
    if journal_mark(&dir).await != JournalFact::Unreadable {
        return Err("dir".to_owned());
    }
    let link = scratch.path().join("link.db");
    let missing = scratch.path().join("missing-target");
    std::os::unix::fs::symlink(&missing, &link).map_err(|err| err.to_string())?;
    if missing.exists() {
        return Err("created".to_owned());
    }
    if journal_mark(&link).await == JournalFact::Unreadable {
        Ok(())
    } else {
        Err("link".to_owned())
    }
}
