//! Execute the planning snapshot observer over real files.

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_actions_contract::CacheSnapshotDomain;
use velnor_actions_mise::cache_snapshot::snapshot_source;

type TestResult = Result<(), Box<dyn Error>>;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "velnor-planning-snapshot-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(root.join("velnor")).expect("owned root");
        Self(root)
    }

    fn put(&self, path: &str, bytes: &[u8]) {
        let target = self.0.join("velnor").join(path);
        fs::create_dir_all(target.parent().expect("parent")).expect("parents");
        fs::write(target, bytes).expect("file");
    }

    fn observe(&self, before: bool, restored: &str) -> TestResult {
        let mut env = CacheSnapshotDomain::PlanningTools.environment(before);
        env.insert("VELNOR_SNAPSHOT_RESTORED".to_owned(), restored.to_owned());
        let source = snapshot_source()?;
        let output = Command::new("/bin/sh")
            .args([
                "-c",
                &source,
                "velnor-snapshot",
                "planning_tools",
                if before { "before" } else { "after" },
            ])
            .envs(env)
            .env("RUNNER_TEMP", &self.0)
            .env("GITHUB_ENV", self.0.join("github-env"))
            .env("GITHUB_OUTPUT", self.0.join("github-output"))
            .output()
            .expect("execute observer");
        assert!(output.status.success(), "observer failed: {output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("export suppressed"));
        Ok(())
    }

    fn outputs(&self) -> String {
        fs::read_to_string(self.0.join("github-env")).unwrap_or_default()
    }

    fn observer_outputs(&self) -> String {
        fs::read_to_string(self.0.join("github-output")).unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("cleanup");
    }
}

#[test]
fn planning_payload_and_restore_marker_cannot_grant_qualification() -> TestResult {
    let fixture = Fixture::new();
    fs::remove_dir(fixture.0.join("velnor")).expect("fresh runner temp");
    fixture.observe(true, "")?;
    fixture.put("planning/mise/gh", b"gh");
    fixture.put("mise/full-tool", b"unselected");
    fixture.observe(false, "")?;
    fixture.observe(true, "claimed-planning-snapshot")?;
    fixture.observe(false, "claimed-planning-snapshot")?;
    let environment = fixture.outputs();
    let output = fixture.observer_outputs();
    assert!(environment.contains("SNAPSHOT_CHANGED=false\n"));
    assert!(environment.contains("SNAPSHOT_DIGEST=\n"));
    assert!(!environment.contains("SNAPSHOT_CHANGED=true\n"));
    assert!(output.contains("available=false\n"));
    assert!(output.contains("changed=false\n"));
    assert!(!output.contains("available=true\n"));
    assert!(!output.contains("digest="));
    Ok(())
}
