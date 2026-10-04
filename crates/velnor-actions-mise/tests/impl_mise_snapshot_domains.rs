#![cfg(unix)]

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use velnor_actions_contract::{CacheSnapshotDomain, ToolCacheDomain};
use velnor_actions_mise::cache_snapshot::snapshot_source;

type TestResult = Result<(), Box<dyn Error>>;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "velnor-tool-domain-snapshot-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).expect("fixture");
        Self(root.canonicalize().expect("canonical root"))
    }
    fn put(&self, relative: &str) -> PathBuf {
        let path = self.0.join("velnor").join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("parents");
        fs::write(&path, b"immutable tool").expect("tool");
        path
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
            .env("GITHUB_ENV", self.0.join("env"))
            .env("GITHUB_OUTPUT", self.0.join("output"))
            .output()
            .expect("observer");
        assert!(output.status.success(), "{output:?}");
        Ok(String::from_utf8(output.stderr)?)
    }
    fn outputs(&self) -> String {
        fs::read_to_string(self.0.join("output")).unwrap_or_default()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("cleanup");
    }
}
fn domains() -> [ToolCacheDomain; 6] {
    [
        ToolCacheDomain::Planning,
        ToolCacheDomain::Full,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ]
}

#[test]
fn typed_tool_observers_cover_exact_canonical_payloads() {
    for domain in domains() {
        let layer = CacheSnapshotDomain::tool_domain(domain);
        let expected: Vec<_> = layer
            .roots()
            .iter()
            .map(|root| format!("${{{{ runner.temp }}}}/velnor/{root}"))
            .collect();
        assert_eq!(expected, domain.payload());
    }
}

#[test]
fn all_tool_domains_suppress_unqualified_payload_and_link_states() -> TestResult {
    for domain in domains() {
        let layer = CacheSnapshotDomain::tool_domain(domain);
        let fixture = Fixture::new();
        assert!(
            fixture
                .observe(layer, true, "")?
                .contains("export suppressed")
        );
        let relative = format!("{}/bin/tool", layer.roots()[0]);
        let tool = fixture.put(&relative);
        let link = tool.with_file_name("link");
        symlink(&tool, &link).expect("owned link");
        assert!(
            fixture
                .observe(layer, false, "")?
                .contains("export suppressed")
        );
        assert!(
            fixture
                .observe(layer, true, "claimed")?
                .contains("export suppressed")
        );
        assert!(
            fixture
                .observe(layer, false, "claimed")?
                .contains("export suppressed")
        );
        fs::remove_file(&link).expect("remove link");
        let foreign = fixture.put("foreign/tool");
        symlink(foreign, &link).expect("foreign link");
        assert!(
            fixture
                .observe(layer, false, "claimed")?
                .contains("export suppressed")
        );
        let output = fixture.outputs();
        let environment = fs::read_to_string(fixture.0.join("env"))?;
        assert!(output.contains("available=false\n"));
        assert!(output.contains("changed=false\n"));
        assert!(!output.contains("available=true\n"));
        assert!(!output.contains("digest="));
        assert!(environment.contains("SNAPSHOT_CHANGED=false\n"));
        assert!(environment.contains("SNAPSHOT_DIGEST=\n"));
        assert!(!environment.contains("SNAPSHOT_CHANGED=true\n"));
    }
    Ok(())
}
