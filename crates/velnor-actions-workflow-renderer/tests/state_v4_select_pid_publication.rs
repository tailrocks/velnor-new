use std::error::Error;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::call_helpers::{clear_readiness_path, read_published_pid};
use super::wait_until;
use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Gh, REPO};
use crate::gc1_seal_units::{group_is_live, process_group, process_is_running};

#[test]
#[cfg(target_os = "linux")]
fn fake_gh_listing_pid_is_absent_until_complete_publication() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("listing-pid-publication", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    gh.mode("list-hang")?;
    let gh_dir = PathBuf::from(gh.dir());
    let ready_path = gh_dir.join("list-ready-publication-probe");
    clear_readiness_path(&ready_path)?;
    let opened_path = gh_dir.join("list-ready-opened");
    let release_path = gh_dir.join("list-ready-release");
    let child = Command::new(scene.scratch.join("bin/gh"))
        .args(["cache", "list", "-R", REPO, "--key", "unused"])
        .current_dir(&scene.workspace)
        .env("FAKE_GH_DIR", &gh_dir)
        .env("FAKE_GH_LIST_PID_READY", &ready_path)
        .env("FAKE_GH_LIST_PID_OPENED", &opened_path)
        .env("FAKE_GH_LIST_PID_RELEASE", &release_path)
        .env("GH_REPO", REPO)
        .env("GITHUB_REPOSITORY", REPO)
        .env("GH_TOKEN", "fixture-token")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut probe = ListingProbe::new(child, release_path);
    wait_until(|| opened_path.exists(), Duration::from_secs(10))?;
    let readiness_visible_while_open = ready_path.exists();
    probe.release()?;
    if readiness_visible_while_open {
        probe.release_and_reap()?;
        scene.finish()?;
        return Err(fail("listing PID readiness is absent before publication"));
    }
    wait_until(|| ready_path.exists(), Duration::from_secs(10))?;
    let published_pid = read_published_pid(&ready_path)?;
    let pid_matches_child = published_pid == probe.pid
        && process_is_running(published_pid)
        && process_group(published_pid) == Some(probe.pid)
        && group_is_live(probe.pid);
    let invocations = std::fs::read_to_string(gh_dir.join("invocations"))?;
    let fake_gh_was_invoked = invocations
        .lines()
        .any(|line| line.starts_with("cache list -R tailrocks/velnor-new"));
    probe.release_and_reap()?;
    if !pid_matches_child || !fake_gh_was_invoked {
        return Err(fail(
            "published PID did not identify the spawned fake GH process",
        ));
    }
    scene.finish()
}

struct ListingProbe {
    child: Child,
    pid: u32,
    release_path: PathBuf,
    reaped: bool,
}

impl ListingProbe {
    fn new(child: Child, release_path: PathBuf) -> Self {
        Self {
            pid: child.id(),
            child,
            release_path,
            reaped: false,
        }
    }

    fn release(&self) -> Result<(), Box<dyn Error>> {
        std::fs::write(&self.release_path, "release\n")?;
        Ok(())
    }

    fn release_and_reap(&mut self) -> Result<(), Box<dyn Error>> {
        self.release()?;
        signal_group(self.pid, "TERM");
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let child_done = self.child.try_wait()?.is_some();
            if child_done && !group_is_live(self.pid) {
                self.reaped = true;
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        signal_group(self.pid, "KILL");
        let _ = self.child.wait()?;
        wait_until(|| !group_is_live(self.pid), Duration::from_secs(2))?;
        self.reaped = true;
        Ok(())
    }
}

impl Drop for ListingProbe {
    fn drop(&mut self) {
        if !self.reaped {
            drop(self.release());
            signal_group(self.pid, "TERM");
            thread::sleep(Duration::from_millis(50));
            signal_group(self.pid, "KILL");
            drop(self.child.wait());
        }
    }
}

fn signal_group(pid: u32, signal: &str) {
    drop(
        Command::new("kill")
            .args([format!("-{signal}"), "--".to_owned(), format!("-{pid}")])
            .status(),
    );
}
