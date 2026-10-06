use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_actions_contract::CacheSnapshotDomain;

use velnor_actions_mise::cache_snapshot::snapshot_source;

type TestResult = Result<(), Box<dyn Error>>;
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    environment: PathBuf,
    output: PathBuf,
}

struct Observation {
    environment: String,
    output: String,
    stderr: String,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = std::env::temp_dir().join(format!(
            "velnor-snapshot-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root)?;
        Ok(Self {
            environment: root.join("github-env"),
            output: root.join("github-output"),
            root,
        })
    }

    fn observe(
        &self,
        domain: CacheSnapshotDomain,
        before: bool,
        restored: &str,
    ) -> Result<Observation, Box<dyn Error>> {
        self.observe_with_bindings(domain, before, restored, &[])
    }

    fn observe_with_bindings(
        &self,
        domain: CacheSnapshotDomain,
        before: bool,
        restored: &str,
        bindings: &[(&str, &str)],
    ) -> Result<Observation, Box<dyn Error>> {
        fs::write(&self.environment, b"")?;
        fs::write(&self.output, b"")?;
        let source = snapshot_source()?;
        let result = Command::new("/bin/sh")
            .args([
                "-c",
                &source,
                "velnor-snapshot",
                domain.name(),
                if before { "before" } else { "after" },
            ])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .envs(domain.environment(before))
            .envs(bindings.iter().copied())
            .env("VELNOR_SNAPSHOT_RESTORED", restored)
            .env("RUNNER_TEMP", &self.root)
            .env("GITHUB_ENV", &self.environment)
            .env("GITHUB_OUTPUT", &self.output)
            .output()?;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        Ok(Observation {
            environment: fs::read_to_string(&self.environment)?,
            output: fs::read_to_string(&self.output)?,
            stderr: String::from_utf8(result.stderr)?,
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("snapshot fixture cleanup failed: {error}");
        }
    }
}

fn available(output: &str) -> bool {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("available="))
        .next_back()
        == Some("true")
}

#[test]
fn universal_snapshot_source_covers_every_closed_domain() -> TestResult {
    let source = snapshot_source()?;
    assert!(!source.contains("@DOMAINS@"));
    assert!(!source.contains("@ENGINE@"));
    assert!(!source.contains("@ENTRY@"));
    assert!(!source.contains("velnor-opaque-inventory-v1"));
    assert!(source.contains("_INVENTORY_TEMPLATE_SHA256"));
    assert!(source.contains("source_archive_inventory(_snapshot_context, None)"));
    for domain in CacheSnapshotDomain::ALL {
        assert!(source.contains(&format!("  {})", domain.name())));
        let environment = domain.environment(false);
        assert_eq!(environment["VELNOR_SNAPSHOT_LAYER"], domain.name());
        assert_eq!(
            environment["VELNOR_SNAPSHOT_ROOTS"],
            domain.roots().join(",")
        );
        assert!(environment["VELNOR_SNAPSHOT_RESTORED"].contains(domain.restore_id()));
    }
    Ok(())
}

#[test]
fn all_twenty_domain_phases_are_unavailable_without_qualification() -> TestResult {
    let mut observations = 0;
    for domain in CacheSnapshotDomain::ALL {
        let fixture = Fixture::new()?;
        for before in [true, false] {
            let observed = fixture.observe(domain, before, "restored-key")?;
            assert_suppressed(&observed);
            observations += 1;
        }
    }
    assert_eq!(observations, 20);
    Ok(())
}

#[test]
fn payload_and_restored_markers_cannot_grant_qualification() -> TestResult {
    let fixture = Fixture::new()?;
    let directory = fixture.root.join("velnor/mise");
    fs::create_dir_all(&directory)?;
    let payload = directory.join("payload\nopaque");
    fs::write(&payload, b"\0\xfffirst")?;
    let domain = CacheSnapshotDomain::Tools;
    assert_suppressed(&fixture.observe(domain, true, "")?);
    for restored in ["", "restored-key"] {
        assert_suppressed(&fixture.observe(domain, false, restored)?);
    }
    fs::write(&payload, b"\0\xffother")?;
    assert_suppressed(&fixture.observe(domain, false, "restored-key")?);
    Ok(())
}

#[test]
fn malformed_typed_bindings_suppress_export_and_clear_digest() -> TestResult {
    let fixture = Fixture::new()?;
    for before in [true, false] {
        for (name, value) in [
            ("VELNOR_SNAPSHOT_PHASE", "invalid"),
            ("VELNOR_SNAPSHOT_LAYER", "unknown"),
            ("VELNOR_SNAPSHOT_OUTPUT", "UNKNOWN"),
            ("VELNOR_SNAPSHOT_ROOTS", "mise,../outside"),
        ] {
            let observed = fixture.observe_with_bindings(
                CacheSnapshotDomain::Tools,
                before,
                "restored-key",
                &[(name, value)],
            )?;
            assert_suppressed(&observed);
            assert!(
                !fixture
                    .root
                    .join("velnor/cache-snapshots/tools-before")
                    .exists()
            );
        }
    }
    Ok(())
}

fn assert_suppressed(observed: &Observation) {
    assert!(!available(&observed.output));
    assert!(observed.output.contains("available=false\n"));
    assert!(observed.output.contains("changed=false\n"));
    assert!(!observed.output.contains("changed=true\n"));
    assert!(!observed.output.contains("available=true\n"));
    assert!(
        !observed
            .output
            .lines()
            .any(|line| line.starts_with("digest="))
    );
    assert_eq!(
        observed
            .environment
            .lines()
            .filter_map(|line| line.split_once("_SNAPSHOT_DIGEST=").map(|(_, value)| value))
            .next_back(),
        Some(""),
    );
    assert_eq!(
        observed
            .environment
            .lines()
            .filter_map(|line| line
                .split_once("_SNAPSHOT_CHANGED=")
                .map(|(_, value)| value))
            .next_back(),
        Some("false"),
    );
    assert!(
        observed
            .stderr
            .contains("snapshot unavailable; export suppressed")
    );
}

#[cfg(unix)]
#[test]
fn unqualified_source_link_state_suppresses_export_before_and_after() -> TestResult {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new()?;
    let directory = fixture.root.join("velnor/cargo/registry/cache");
    fs::create_dir_all(&directory)?;
    fs::write(directory.join("archive"), b"opaque")?;
    symlink("archive", directory.join("linked-archive"))?;
    for before in [true, false] {
        let observed = fixture.observe(CacheSnapshotDomain::Sources, before, "")?;
        assert_suppressed(&observed);
        assert!(
            observed
                .environment
                .contains("VELNOR_SOURCES_SNAPSHOT_CHANGED=false\n")
        );
    }
    Ok(())
}
