//! Execute the emitted fixed observer over real files, including repair cases.

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
            "velnor-snapshot-script-{}-{}",
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

    fn observe(
        &self,
        layer: CacheSnapshotDomain,
        before: bool,
        restored: &str,
    ) -> Result<String, Box<dyn Error>> {
        let mut env = layer.environment(before);
        env.insert("VELNOR_SNAPSHOT_RESTORED".to_owned(), restored.to_owned());
        let source = snapshot_source()?;
        let output = Command::new("/bin/sh")
            .args([
                "-c",
                &source,
                "velnor-snapshot",
                layer.name(),
                if before { "before" } else { "after" },
            ])
            .envs(env)
            .env("RUNNER_TEMP", &self.0)
            .env("GITHUB_ENV", self.0.join("github-env"))
            .env("GITHUB_OUTPUT", self.0.join("github-output"))
            .output()
            .expect("execute fixed script");
        assert!(
            output.status.success(),
            "bookkeeping must preserve outcome: {output:?}"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("export suppressed"));
        assert_unqualified(self);
        Ok(format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }

    fn outputs(&self) -> String {
        fs::read_to_string(self.0.join("github-env")).unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("cleanup");
    }
}

#[test]
fn payload_restore_and_repair_cannot_grant_qualification() -> TestResult {
    let fresh = Fixture::new();
    fs::remove_dir(fresh.0.join("velnor")).expect("truly fresh runner temp");
    fresh.observe(CacheSnapshotDomain::Tools, true, "")?;
    fresh.put("rustup/toolchains/exact/compiler", b"compiler");
    fresh.put("cargo/bin/proxy", b"proxy");
    fresh.observe(CacheSnapshotDomain::Tools, false, "")?;
    assert_unqualified(&fresh);

    let claimed_restore = Fixture::new();
    claimed_restore.put("rustup/toolchains/exact/compiler", b"compiler");
    claimed_restore.put("cargo/bin/proxy", b"proxy");
    claimed_restore.observe(CacheSnapshotDomain::Tools, true, "claimed-snapshot")?;
    claimed_restore.observe(CacheSnapshotDomain::Tools, false, "claimed-snapshot")?;
    assert!(claimed_restore.outputs().contains("SNAPSHOT_CHANGED=false"));
    assert_eq!(
        fresh.outputs().lines().next(),
        claimed_restore.outputs().lines().next()
    );

    fs::remove_file(claimed_restore.0.join("velnor/cargo/bin/proxy")).expect("damage");
    claimed_restore.observe(CacheSnapshotDomain::Tools, true, "claimed-snapshot")?;
    claimed_restore.put("cargo/bin/proxy", b"proxy");
    claimed_restore.observe(CacheSnapshotDomain::Tools, false, "claimed-snapshot")?;
    assert_unqualified(&claimed_restore);

    let another_fixture = Fixture::new();
    another_fixture.put("rustup/toolchains/exact/compiler", b"compiler");
    another_fixture.put("cargo/bin/proxy", b"proxy");
    another_fixture.observe(
        CacheSnapshotDomain::Tools,
        true,
        "claimed-repaired-snapshot",
    )?;
    another_fixture.observe(
        CacheSnapshotDomain::Tools,
        false,
        "claimed-repaired-snapshot",
    )?;
    assert!(another_fixture.outputs().contains("SNAPSHOT_CHANGED=false"));
    Ok(())
}

#[test]
fn empty_unqualified_payload_never_exports() -> TestResult {
    let fixture = Fixture::new();
    fixture.observe(CacheSnapshotDomain::Sources, true, "")?;
    fixture.observe(CacheSnapshotDomain::Sources, false, "")?;
    assert!(fixture.outputs().contains("SNAPSHOT_CHANGED=false"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn unqualified_owned_and_external_link_states_suppress_export() -> TestResult {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    fixture.put("cargo/bin/rustup", b"proxy target");
    symlink("missing", fixture.0.join("velnor/cargo/bin/rustc")).expect("damaged link");
    fixture.observe(CacheSnapshotDomain::Tools, true, "damaged-snapshot")?;
    fs::remove_file(fixture.0.join("velnor/cargo/bin/rustc")).expect("remove damaged");
    symlink("rustup", fixture.0.join("velnor/cargo/bin/rustc")).expect("repair link");
    fixture.observe(CacheSnapshotDomain::Tools, false, "damaged-snapshot")?;
    assert_unqualified(&fixture);
    fs::remove_file(fixture.0.join("velnor/cargo/bin/rustc")).expect("remove owned");
    symlink("/etc/passwd", fixture.0.join("velnor/cargo/bin/rustc")).expect("outside target");
    fixture.observe(CacheSnapshotDomain::Tools, true, "outside-snapshot")?;
    assert!(
        fixture
            .observe(CacheSnapshotDomain::Tools, false, "outside-snapshot")?
            .contains("export suppressed")
    );
    Ok(())
}

#[test]
fn source_delta_and_unowned_state_remain_unqualified() -> TestResult {
    let fixture = Fixture::new();
    fixture.put("cargo/registry/cache/public/a.crate", b"archive");
    fixture.observe(CacheSnapshotDomain::Sources, true, "source-snapshot")?;
    fixture.put("cargo/credentials.toml", b"credential excluded");
    fixture.put("cargo/registry/src/extracted/lib.rs", b"reconstructed");
    fixture.observe(CacheSnapshotDomain::Sources, false, "source-snapshot")?;
    assert!(fixture.outputs().contains("SNAPSHOT_CHANGED=false"));
    fixture.observe(CacheSnapshotDomain::Sources, true, "source-snapshot")?;
    fixture.put(
        "cargo/registry/cache/public/target.crate",
        b"new requirement",
    );
    fixture.observe(CacheSnapshotDomain::Sources, false, "source-snapshot")?;
    assert_unqualified(&fixture);
    Ok(())
}

#[test]
fn bun_member_changes_remain_unqualified() -> TestResult {
    let fixture = Fixture::new();
    fixture.put("native/bun/install/cache/package/index.js", b"package");
    fixture.observe(CacheSnapshotDomain::BunDownloads, true, "bun-snapshot")?;
    fixture.put("native/bun/install/cache/package/.git/config", b"fixture");
    fixture.put("native/bun/install/cache/package/.tmp/fixture", b"fixture");
    fixture.observe(CacheSnapshotDomain::BunDownloads, false, "bun-snapshot")?;
    assert_unqualified(&fixture);
    Ok(())
}

#[test]
fn npm_proof_changes_remain_unqualified() -> TestResult {
    let fixture = Fixture::new();
    fixture.put("native/npm/_cacache/content-v2/public", b"public archive");
    fixture.put("native/npm/public-proof-v1.json", b"old proof");
    fixture.observe(CacheSnapshotDomain::NpmDownloads, true, "npm-snapshot")?;
    fixture.put("native/npm/public-proof-v1.json", b"repaired proof");
    fixture.observe(CacheSnapshotDomain::NpmDownloads, false, "npm-snapshot")?;
    assert_unqualified(&fixture);
    Ok(())
}

#[cfg(unix)]
#[test]
fn ancestor_links_suppress_export_without_failing_work() -> TestResult {
    use std::os::unix::fs::symlink;
    let escaped = Fixture::new();
    symlink("/etc", escaped.0.join("velnor/cargo")).expect("ancestor link");
    assert!(
        escaped
            .observe(CacheSnapshotDomain::Sources, true, "hit")?
            .contains("export suppressed")
    );
    assert!(escaped.outputs().contains("SNAPSHOT_CHANGED=false"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn source_links_suppress_export_in_both_phases() -> TestResult {
    use std::os::unix::fs::symlink;
    for before in [true, false] {
        let fixture = Fixture::new();
        fixture.put("cargo/registry/cache/public/archive", b"archive");
        symlink(
            "archive",
            fixture.0.join("velnor/cargo/registry/cache/public/link"),
        )
        .expect("source link");
        assert!(
            fixture
                .observe(CacheSnapshotDomain::Sources, before, "source-snapshot")?
                .contains("export suppressed")
        );
        assert!(fixture.outputs().contains("SNAPSHOT_CHANGED=false"));
    }
    Ok(())
}

fn assert_unqualified(fixture: &Fixture) {
    let output = fs::read_to_string(fixture.0.join("github-output")).expect("output");
    let environment = fixture.outputs();
    assert!(output.contains("available=false\n"));
    assert!(output.contains("changed=false\n"));
    assert!(!output.contains("available=true\n"));
    assert!(!output.contains("digest="));
    assert!(environment.contains("SNAPSHOT_CHANGED=false\n"));
    assert!(environment.contains("SNAPSHOT_DIGEST=\n"));
    assert!(!environment.contains("SNAPSHOT_CHANGED=true\n"));
}
