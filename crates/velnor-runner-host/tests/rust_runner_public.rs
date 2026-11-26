// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use velnor_runner_host::{
    HostConfig, HostError, LaunchMode, RunningJob, RustRunnerFailure, RustRunnerLauncher,
};

const FAKE_ORB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake_orb.sh");
const SECRET_JIT: &str = "eyJ2ZWxub3IiOiJzZWNyZXQtaml0LWNvbmZpZyJ9";

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    store: PathBuf,
}

impl Fixture {
    fn new(machines: &str) -> Result<Self, Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let root = directory.path().canonicalize()?;
        let store = root.join("store");
        std::fs::create_dir_all(root.join("seen"))?;
        std::fs::create_dir_all(store.join("bin"))?;
        std::fs::copy(FAKE_ORB, root.join("orb"))?;
        std::fs::write(root.join("list.json"), machines)?;
        let binary = store.join("bin/velnor-runner");
        std::fs::write(&binary, "")?;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))?;
        let fixture = Self {
            _directory: directory,
            root,
            store,
        };
        fixture.configure("")?;
        Ok(fixture)
    }

    fn configure(&self, extra: &str) -> Result<(), Box<dyn Error>> {
        let env = format!(
            "FAKE_ORB_LOG={log}\nFAKE_ORB_LIST={list}\nFAKE_ORB_STORE={store}\nFAKE_ORB_SEEN={seen}\n{extra}",
            log = self.root.join("orb.log").display(),
            list = self.root.join("list.json").display(),
            store = self.store.display(),
            seen = self.root.join("seen").display(),
        );
        std::fs::write(self.root.join("fake_orb.env"), env)?;
        Ok(())
    }

    fn launcher(&self) -> RustRunnerLauncher {
        RustRunnerLauncher::new(self.root.join("orb"), self.store.clone())
    }

    fn log(&self) -> Result<Vec<String>, Box<dyn Error>> {
        match std::fs::read_to_string(self.root.join("orb.log")) {
            Ok(text) => Ok(text.lines().map(str::to_owned).collect()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }

    fn seen(&self, name: &str) -> PathBuf {
        self.root.join("seen").join(name)
    }

    fn jit_file(&self, reservation: i64) -> PathBuf {
        self.store.join("jit").join(format!("{reservation}.jit"))
    }
}

fn runner_lines(log: &[String]) -> Vec<&String> {
    log.iter()
        .filter(|line| line.contains("/var/lib/velnor/bin/velnor-runner run"))
        .collect()
}

fn wait_for(path: &Path) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if Instant::now() > deadline {
            return Err(format!("{} never appeared", path.display()).into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn process_exists(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn rust_runner_creates_missing_machine() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(r#"[{"name":"velnor-other"},{"name":"jackin"}]"#)?;
    fixture.launcher().ensure_machine()?;
    let log = fixture.log()?;
    assert_eq!(log[0], "list -f json");
    assert!(
        log.contains(&"create -a arm64 ubuntu velnor".to_owned()),
        "{log:?}"
    );
    fixture.launcher().ensure_machine()?;
    let creates = fixture
        .log()?
        .iter()
        .filter(|line| line.starts_with("create"))
        .count();
    assert_eq!(creates, 1);
    Ok(())
}

#[test]
fn rust_runner_skips_create_when_machine_exists() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(r#"[{"name":"jackin"},{"name":"velnor"}]"#)?;
    fixture.launcher().ensure_machine()?;
    assert!(
        fixture
            .log()?
            .iter()
            .all(|line| !line.starts_with("create")),
        "{:?}",
        fixture.log()?
    );
    Ok(())
}

#[test]
fn rust_runner_links_var_lib_velnor_to_host_store() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new("[]")?;
    fixture.launcher().ensure_machine()?;
    let expected = format!(
        "-m velnor -u root ln -sfn /mnt/mac{} /var/lib/velnor",
        fixture.store.display()
    );
    assert!(fixture.log()?.contains(&expected), "{:?}", fixture.log()?);
    for name in [
        "bin", "crates", "tools", "git", "gen", "cache", "records", "jit",
    ] {
        assert!(fixture.store.join(name).is_dir(), "{name}");
    }
    Ok(())
}

#[test]
fn rust_runner_starts_run_jit_per_job_with_distinct_files() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(r#"[{"name":"velnor"}]"#)?;
    let launcher = fixture.launcher();
    let first = launcher.start_job(11, "first-jit")?;
    let second = launcher.start_job(12, "second-jit")?;
    assert!(first.wait()?.success());
    assert!(second.wait()?.success());

    let log = fixture.log()?;
    let lines = runner_lines(&log);
    assert_eq!(lines.len(), 2, "{log:?}");
    assert!(
        lines.contains(
            &&"-m velnor unshare --user --map-current-user --pid --fork --kill-child /var/lib/velnor/bin/velnor-runner run --jit @/var/lib/velnor/jit/11.jit"
                .to_owned()
        ),
        "{lines:?}"
    );
    assert!(
        lines.contains(
            &&"-m velnor unshare --user --map-current-user --pid --fork --kill-child /var/lib/velnor/bin/velnor-runner run --jit @/var/lib/velnor/jit/12.jit"
                .to_owned()
        ),
        "{lines:?}"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.seen("11.jit"))?,
        "first-jit"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.seen("12.jit"))?,
        "second-jit"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.seen("11.jit.mode"))?.trim(),
        "600"
    );
    Ok(())
}

#[test]
fn rust_runner_keeps_jit_out_of_argv() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(r#"[{"name":"velnor"}]"#)?;
    let job = fixture.launcher().start_job(21, SECRET_JIT)?;
    assert!(job.wait()?.success());

    assert_eq!(std::fs::read_to_string(fixture.seen("21.jit"))?, SECRET_JIT);
    assert!(
        fixture.log()?.iter().all(|line| !line.contains(SECRET_JIT)),
        "{:?}",
        fixture.log()?
    );
    assert!(!std::fs::read_to_string(fixture.seen("21.jit.env"))?.contains(SECRET_JIT));
    Ok(())
}

#[test]
fn rust_runner_removes_jit_file_after_exit() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(r#"[{"name":"velnor"}]"#)?;
    fixture.configure("FAKE_RUNNER_EXIT=3\n")?;
    let job = fixture.launcher().start_job(31, SECRET_JIT)?;
    let status = job.wait()?;
    assert_eq!(status.code(), Some(3));
    assert!(!fixture.jit_file(31).exists());

    fixture.configure("FAKE_RUNNER_HOLD=1\n")?;
    let job: RunningJob = fixture.launcher().start_job(32, SECRET_JIT)?;
    let child_file = fixture.seen("32.jit.child");
    wait_for(&child_file)?;
    let child = std::fs::read_to_string(&child_file)?.trim().to_owned();
    assert!(fixture.jit_file(32).exists());
    assert!(process_exists(&child));
    job.stop()?;
    assert!(!fixture.jit_file(32).exists());
    let deadline = Instant::now() + Duration::from_secs(10);
    while process_exists(&child) {
        assert!(Instant::now() < deadline, "child {child} survived stop");
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

#[test]
fn rust_runner_fails_when_binary_missing() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(r#"[{"name":"velnor"}]"#)?;
    std::fs::remove_file(fixture.store.join("bin/velnor-runner"))?;
    let error = fixture
        .launcher()
        .ensure_machine()
        .err()
        .ok_or("ensure_machine accepted a missing binary")?;
    assert_eq!(
        error,
        HostError::RustRunner(RustRunnerFailure::BinaryMissing)
    );
    assert_eq!(
        error.to_string(),
        "velnor-runner is missing at /var/lib/velnor/bin/velnor-runner in the velnor machine"
    );
    Ok(())
}

fn host_config(launch_mode: Option<&str>, platform: &str, name: &str, labels: &[&str]) -> String {
    let labels = labels
        .iter()
        .map(|label| format!("\"{label}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let launch_mode =
        launch_mode.map_or_else(String::new, |mode| format!("launch_mode = \"{mode}\"\n"));
    format!(
        "schema = 2\n\
         [[connections]]\n\
         connection_id = \"lab\"\n\
         repository_id = 42\n\
         repository = \"tailrocks/velnor-lab\"\n\
         credential_ref = \"keychain:com.tailrocks.velnor.host/lab\"\n\
         [connections.scale_set]\n\
         name = \"{name}\"\n\
         labels = [{labels}]\n\
         [connections.source_policy]\n\
         mode = \"branch_restricted\"\n\
         branches = [\"main\"]\n\
         tag_prefixes = []\n\
         events = [\"push\"]\n\
         pull_request_rules = []\n\
         [host]\n\
         {launch_mode}\
         max_jobs = 1\n\
         [host.resources]\n\
         runner_cpu_millicores = 2000\n\
         runner_memory_bytes = 4294967296\n\
         dind_cpu_millicores = 1000\n\
         dind_memory_bytes = 2147483648\n\
         operational_memory_reserve_bytes = 1073741824\n\
         [docker]\n\
         context = \"orbstack\"\n\
         platform = \"{platform}\"\n\
         endpoint = \"unix:///var/run/docker.sock\"\n"
    )
}

const MAC_LABELS: [&str; 2] = ["velnor", "velnor-mac"];

#[test]
fn launch_mode_defaults_to_official_runner() -> Result<(), HostError> {
    let parsed = HostConfig::parse(&host_config(
        None,
        "linux/amd64",
        "ubuntu-26.04-scale-set",
        &["velnor", "ubuntu-26.04-scale-set"],
    ))?;
    assert_eq!(parsed.host.launch_mode, LaunchMode::OfficialRunner);
    assert!(
        parsed
            .to_toml()?
            .contains("launch_mode = \"official-runner\"")
    );
    Ok(())
}

#[test]
fn launch_mode_rust_runner_parses() -> Result<(), HostError> {
    let parsed = HostConfig::parse(&host_config(
        Some("rust-runner"),
        "linux/arm64",
        "velnor-mac",
        &MAC_LABELS,
    ))?;
    assert_eq!(parsed.host.launch_mode, LaunchMode::RustRunner);
    let reparsed = HostConfig::parse(&parsed.to_toml()?)?;
    assert_eq!(reparsed.host.launch_mode, LaunchMode::RustRunner);
    Ok(())
}

#[test]
fn launch_mode_unknown_value_is_rejected() {
    for mode in ["docker", "rust_runner", "Rust-Runner", ""] {
        assert_eq!(
            HostConfig::parse(&host_config(
                Some(mode),
                "linux/arm64",
                "velnor-mac",
                &MAC_LABELS
            ))
            .err(),
            Some(HostError::Config),
            "{mode}"
        );
    }
}

#[test]
fn rust_runner_mode_accepts_velnor_mac_selector_and_requires_arm64() {
    assert!(
        HostConfig::parse(&host_config(
            Some("rust-runner"),
            "linux/arm64",
            "velnor-mac",
            &MAC_LABELS
        ))
        .is_ok()
    );
    assert_eq!(
        HostConfig::parse(&host_config(
            Some("rust-runner"),
            "linux/amd64",
            "velnor-mac",
            &MAC_LABELS
        ))
        .err(),
        Some(HostError::Config)
    );
    assert_eq!(
        HostConfig::parse(&host_config(
            Some("official-runner"),
            "linux/arm64",
            "velnor-mac",
            &MAC_LABELS
        ))
        .err(),
        Some(HostError::PlatformSelector)
    );
}
