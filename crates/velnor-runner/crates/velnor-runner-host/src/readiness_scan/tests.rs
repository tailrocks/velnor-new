use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{ConfigFact, JournalFact, assess, classify_engine_journal, journal_mark, read_config};
use crate::journal::{Journal, Outcome};
use crate::readiness::Readiness;

const SERVICE: &str = "com.tailrocks.velnor.readiness-test";
const ACCOUNT: &str = "absent";

fn deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + super::READINESS_BUDGET
}

#[test]
fn partial_observer_never_claims_ready() {
    assert_eq!(
        classify_engine_journal(true, JournalFact::Clear),
        Readiness::Reconciling
    );
    assert_eq!(
        classify_engine_journal(true, JournalFact::Occupied),
        Readiness::Reconciling
    );
    assert_eq!(
        classify_engine_journal(true, JournalFact::Unreadable),
        Readiness::Degraded
    );
    assert_eq!(
        classify_engine_journal(false, JournalFact::Clear),
        Readiness::WaitingForEngine
    );
}

struct Scratch {
    path: PathBuf,
}

fn sample() -> String {
    "schema = 1\n[github]\nrepository = \"example/repo\"\nscale_set_name = \"ubuntu-26.04-scale-set\"\ncredential_ref = \"keychain:test/absent\"\n[host]\nmax_jobs = 1\n[host.resources]\nrunner_cpu_millicores = 1000\nrunner_memory_bytes = 2147483648\ndind_cpu_millicores = 3000\ndind_memory_bytes = 6442450944\n[docker]\ncontext = \"test\"\nplatform = \"linux/amd64\"\nendpoint = \"unix:///tmp/velnor-readiness-absent.sock\"\n".to_owned()
}

#[test]
fn missing_state_remains_missing() -> Result<(), String> {
    let scratch = Scratch::new("missing-state")?;
    let missing = scratch.path().join("missing");
    let state = super::controller_readiness(&missing, SERVICE, ACCOUNT);
    if missing.exists() {
        return Err("readiness created the missing state directory".to_owned());
    }
    if state == Readiness::WaitingForCredentials {
        Ok(())
    } else {
        Err(state.as_str().to_owned())
    }
}

#[tokio::test]
async fn drain_file_reports_draining_without_config() -> Result<(), String> {
    let scratch = Scratch::new("drain")?;
    std::fs::write(scratch.path().join("drain"), b"1").map_err(|err| err.to_string())?;
    let state = assess(scratch.path(), SERVICE, ACCOUNT, deadline()).await;
    if state == Readiness::Draining {
        Ok(())
    } else {
        Err(state.as_str().to_owned())
    }
}

#[tokio::test]
async fn missing_keychain_item_beats_a_dead_socket() -> Result<(), String> {
    let scratch = Scratch::new("cred")?;
    std::fs::write(scratch.path().join("host.toml"), sample()).map_err(|err| err.to_string())?;
    let state = assess(scratch.path(), SERVICE, ACCOUNT, deadline()).await;
    if state == Readiness::WaitingForCredentials {
        Ok(())
    } else {
        Err(state.as_str().to_owned())
    }
}

#[cfg(unix)]
#[test]
fn nonregular_config_is_rejected_without_reading() -> Result<(), String> {
    let scratch = Scratch::new("config-fifo")?;
    let status = std::process::Command::new("mkfifo")
        .arg(scratch.path().join("host.toml"))
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("mkfifo returned {status}"));
    }
    if matches!(read_config(scratch.path()), ConfigFact::Invalid) {
        Ok(())
    } else {
        Err("FIFO config was admitted".to_owned())
    }
}

#[cfg(unix)]
#[test]
fn symlinked_config_is_rejected() -> Result<(), String> {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new("config-link")?;
    let outside = scratch.path().join("outside.toml");
    let state = scratch.path().join("state");
    std::fs::create_dir(&state).map_err(|error| error.to_string())?;
    std::fs::write(&outside, sample()).map_err(|error| error.to_string())?;
    symlink(outside, state.join("host.toml")).map_err(|error| error.to_string())?;
    if matches!(read_config(&state), ConfigFact::Invalid) {
        Ok(())
    } else {
        Err("symlinked config was admitted".to_owned())
    }
}

#[test]
fn oversized_config_is_rejected() -> Result<(), String> {
    let scratch = Scratch::new("config-large")?;
    let size = usize::try_from(super::MAX_CONFIG_BYTES).map_err(|error| error.to_string())?;
    std::fs::write(
        scratch.path().join("host.toml"),
        " ".repeat(size.saturating_add(1)),
    )
    .map_err(|error| error.to_string())?;
    if matches!(read_config(scratch.path()), ConfigFact::Invalid) {
        Ok(())
    } else {
        Err("oversized config was admitted".to_owned())
    }
}

impl Scratch {
    fn new(label: &str) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-ready-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir(&path).map_err(|err| err.to_string())?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn cleanup(&self) -> Result<(), String> {
        match std::fs::remove_dir_all(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("scratch cleanup failed: {error}")),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "failed to remove readiness scratch {}: {error}",
                self.path.display()
            );
        }
    }
}

#[tokio::test]
async fn journal_scan_stops_after_the_row_budget() -> Result<(), String> {
    let scratch = Scratch::new("row-budget")?;
    let path = scratch.path().join("launch.db");
    let journal = Journal::open(&path).await.map_err(|_| "open")?;
    drop(journal);
    let text = path.to_str().ok_or("utf8")?;
    let db = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|_| "build")?;
    let conn = db.connect().map_err(|_| "connect")?;
    conn.execute(
        "WITH RECURSIVE seq(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM seq WHERE x < 4096) INSERT INTO intents (kind, subject, state, cleanup_proven) SELECT 'acquire', CAST(x AS TEXT), 'failed', 1 FROM seq",
        (),
    )
    .await
    .map_err(|_| "insert rows")?;
    if journal_mark(&path, deadline()).await != JournalFact::Clear {
        return Err("exact row budget was rejected".to_owned());
    }
    conn.execute(
        "INSERT INTO intents (kind, subject, state, cleanup_proven) VALUES ('launch', 'overflow', 'pending', 0)",
        (),
    )
    .await
    .map_err(|_| "insert occupying overflow row")?;
    drop(conn);
    if journal_mark(&path, deadline()).await == JournalFact::Unreadable {
        scratch.cleanup()?;
        Ok(())
    } else {
        Err("journal over the row budget was accepted".to_owned())
    }
}

#[tokio::test]
async fn missing_journal_stays_clear_and_uncreated() -> Result<(), String> {
    let scratch = Scratch::new("absent")?;
    let path = scratch.path().join("launch.db");
    let mark = journal_mark(&path, deadline()).await;
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
    if journal_mark(&path, deadline()).await == JournalFact::Unreadable {
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
    if journal_mark(&launch, deadline()).await != JournalFact::Occupied {
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
    if journal_mark(&acquire_path, deadline()).await == JournalFact::Clear {
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
    if journal_mark(&proven, deadline()).await != JournalFact::Clear {
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
    if journal_mark(&done, deadline()).await == JournalFact::Occupied {
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
    if journal_mark(&path, deadline()).await == JournalFact::Unreadable {
        Ok(())
    } else {
        Err("state".to_owned())
    }
}

mod journal_fail_closed;
