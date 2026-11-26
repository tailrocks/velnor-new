// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use velnor_runner_github::EncodedJit;
use velnor_runner_host::{
    HostConfig, HostError, LaunchMode, ReservationId, RunningJob, RustRunnerFailure,
    RustRunnerLauncher,
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
            "FAKE_ORB_LOG={log}\nFAKE_ORB_LIST={list}\nFAKE_ORB_STORE={store}\nFAKE_ORB_SEEN={seen}\nFAKE_ORB_PS={ps}\n{extra}",
            log = self.root.join("orb.log").display(),
            list = self.root.join("list.json").display(),
            store = self.store.display(),
            seen = self.root.join("seen").display(),
            ps = self.root.join("ps.table").display(),
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

    fn jit_file(&self, reservation: u64) -> PathBuf {
        self.store.join("jit").join(format!("{reservation}.jit"))
    }
}

fn reservation(value: u64) -> Result<ReservationId, Box<dyn Error>> {
    ReservationId::new(value).ok_or_else(|| "zero reservation".into())
}

fn jit(text: &str) -> EncodedJit {
    EncodedJit::new(text.to_owned())
}

fn runner_lines(log: &[String]) -> Vec<&String> {
    log.iter()
        .filter(|line| line.contains("/var/lib/velnor/bin/velnor-runner run"))
        .collect()
}

async fn wait_for(path: &Path) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if Instant::now() > deadline {
            return Err(format!("{} never appeared", path.display()).into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
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
        "bin", "crates", "tools", "git", "gen", "cache", "records", "jit", "work",
    ] {
        assert!(fixture.store.join(name).is_dir(), "{name}");
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_starts_run_jit_per_job_with_distinct_files() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(r#"[{"name":"velnor"}]"#)?;
    let launcher = fixture.launcher();
    let mut first = launcher.start_job(reservation(11)?, &jit("first-jit"))?;
    let mut second = launcher.start_job(reservation(12)?, &jit("second-jit"))?;
    assert!(first.wait().await?.success());
    assert!(second.wait().await?.success());

    let log = fixture.log()?;
    let lines = runner_lines(&log);
    assert_eq!(lines.len(), 2, "{log:?}");
    assert!(
        lines.contains(
            &&"-m velnor unshare --user --map-current-user --pid --fork --kill-child /var/lib/velnor/bin/velnor-runner run --jit @/var/lib/velnor/jit/11.jit --work-dir /var/lib/velnor/work/11"
                .to_owned()
        ),
        "{lines:?}"
    );
    assert!(
        lines.contains(
            &&"-m velnor unshare --user --map-current-user --pid --fork --kill-child /var/lib/velnor/bin/velnor-runner run --jit @/var/lib/velnor/jit/12.jit --work-dir /var/lib/velnor/work/12"
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

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_keeps_jit_out_of_argv() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(r#"[{"name":"velnor"}]"#)?;
    let mut job = fixture
        .launcher()
        .start_job(reservation(21)?, &jit(SECRET_JIT))?;
    assert!(job.wait().await?.success());

    assert_eq!(std::fs::read_to_string(fixture.seen("21.jit"))?, SECRET_JIT);
    assert!(
        fixture.log()?.iter().all(|line| !line.contains(SECRET_JIT)),
        "{:?}",
        fixture.log()?
    );
    assert!(!std::fs::read_to_string(fixture.seen("21.jit.env"))?.contains(SECRET_JIT));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_removes_jit_file_after_exit() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(r#"[{"name":"velnor"}]"#)?;
    fixture.configure("FAKE_RUNNER_EXIT=3\n")?;
    let mut job = fixture
        .launcher()
        .start_job(reservation(31)?, &jit(SECRET_JIT))?;
    let status = job.wait().await?;
    assert_eq!(status.code(), Some(3));
    assert!(!fixture.jit_file(31).exists());

    fixture.configure("FAKE_RUNNER_HOLD=1\n")?;
    let job: RunningJob = fixture
        .launcher()
        .start_job(reservation(32)?, &jit(SECRET_JIT))?;
    let child_file = fixture.seen("32.jit.child");
    wait_for(&child_file).await?;
    let child = std::fs::read_to_string(&child_file)?.trim().to_owned();
    assert!(fixture.jit_file(32).exists());
    assert!(process_exists(&child));
    job.stop().await?;
    assert!(!fixture.jit_file(32).exists());
    wait_until_dead(&child).await?;

    let job = fixture
        .launcher()
        .start_job(reservation(33)?, &jit(SECRET_JIT))?;
    let pid_file = fixture.seen("33.jit.pid");
    wait_for(&pid_file).await?;
    wait_for(&fixture.seen("33.jit.child")).await?;
    let pid = std::fs::read_to_string(&pid_file)?.trim().to_owned();
    assert!(fixture.jit_file(33).exists());
    drop(job);
    wait_until_dead(&pid).await?;
    assert!(!fixture.jit_file(33).exists());
    let orphan = std::fs::read_to_string(fixture.seen("33.jit.child"))?
        .trim()
        .to_owned();
    std::process::Command::new("kill").arg(&orphan).status()?;
    wait_until_dead(&orphan).await
}

async fn wait_until_dead(pid: &str) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while process_exists(pid) {
        if Instant::now() > deadline {
            return Err(format!("process {pid} survived").into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
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

const CHILD_MARKER: &str = "VELNOR_RUST_RUNNER_PUBLIC_CHILD";

fn rerun(
    test: &str,
    shell_prefix: &str,
    environment: &[(&str, &str)],
) -> Result<(), Box<dyn Error>> {
    let executable = std::env::current_exe()?;
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{shell_prefix} exec \"$0\" --exact {test} --nocapture"
        ))
        .arg(&executable)
        .env(CHILD_MARKER, test)
        .envs(environment.iter().copied())
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("running 1 test"), "{stdout}\n{stderr}");
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed"),
        "{stdout}\n{stderr}"
    );
    Ok(())
}

fn is_child_of(test: &str) -> bool {
    std::env::var(CHILD_MARKER).is_ok_and(|marker| marker == test)
}

fn environment_keys_of_a_clean_shell() -> Result<Vec<String>, Box<dyn Error>> {
    let output = std::process::Command::new("env")
        .args(["-i", "sh", "-c", "env"])
        .output()?;
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .filter_map(|line| line.split_once('=').map(|(key, _)| key.to_owned()))
        .collect())
}

fn environment_of_call(path: &Path) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    Ok(std::fs::read_to_string(path)?
        .lines()
        .filter_map(|line| {
            line.split_once('=')
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
        })
        .collect())
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_orb_calls_clear_the_environment_and_never_set_orbenv()
-> Result<(), Box<dyn Error>> {
    const TEST: &str = "rust_runner_orb_calls_clear_the_environment_and_never_set_orbenv";
    if !is_child_of(TEST) {
        let path = format!(
            "{}:/rust-runner-l1-extra",
            std::env::var("PATH").unwrap_or_default()
        );
        return rerun(
            TEST,
            "",
            &[
                ("ORBENV", "PAT-CANARY-L1-ORBENV"),
                ("GH_TOKEN", "PAT-CANARY-L1-GHTOKEN"),
                ("VELNOR_SECRET", "PAT-CANARY-L1-SECRET"),
                ("HOME", "/rust-runner-l1-home"),
                ("PATH", &path),
            ],
        );
    }
    let fixture = Fixture::new("[]")?;
    let launcher = fixture.launcher();
    launcher.ensure_machine()?;
    let mut job = launcher.start_job(reservation(41)?, &jit(SECRET_JIT))?;
    assert!(job.wait().await?.success());
    fixture.configure("FAKE_RUNNER_HOLD=1\n")?;
    let held = launcher.start_job(reservation(42)?, &jit(SECRET_JIT))?;
    wait_for(&fixture.seen("42.jit.pid")).await?;
    held.stop().await?;
    std::fs::write(fixture.root.join("ps.table"), "4242 sleep 600\n")?;
    assert_eq!(launcher.list_processes()?.len(), 1);
    launcher.kill_processes(&[4242])?;

    let log = fixture.log()?;
    for expected in [
        "list -f json",
        "create -a arm64 ubuntu velnor",
        "-m velnor -u root ln -sfn",
        "-m velnor test -x /var/lib/velnor/bin/velnor-runner",
        "/var/lib/velnor/bin/velnor-runner run --jit @/var/lib/velnor/jit/41.jit",
        "-m velnor -u root ps -eo pid=,args=",
        "-m velnor -u root kill -9 4242",
    ] {
        assert!(
            log.iter().any(|line| line.contains(expected)),
            "{expected}: {log:?}"
        );
    }
    assert_environments_are_cleared(&fixture)
}

fn assert_environments_are_cleared(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let allowed = {
        let mut keys = environment_keys_of_a_clean_shell()?;
        keys.extend(["PATH".to_owned(), "HOME".to_owned()]);
        keys
    };
    let host_path = std::env::var("PATH")?;
    let host_home = std::env::var("HOME")?;
    let calls = std::fs::read_dir(fixture.seen("calls"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "env"))
        .collect::<Vec<_>>();
    assert!(calls.len() >= fixture.log()?.len(), "{}", calls.len());
    for call in calls {
        let environment = environment_of_call(&call)?;
        for (key, value) in &environment {
            assert!(
                allowed.contains(key),
                "{key} reached orb in {}",
                call.display()
            );
            assert_ne!(key, "ORBENV", "{}", call.display());
            assert!(!value.contains("PAT-CANARY"), "{key} in {}", call.display());
        }
        let value_of = |name: &str| {
            environment
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(
            value_of("PATH"),
            Some(host_path.as_str()),
            "{}",
            call.display()
        );
        assert_eq!(
            value_of("HOME"),
            Some(host_home.as_str()),
            "{}",
            call.display()
        );
    }
    Ok(())
}

#[test]
fn rust_runner_jit_directory_is_private() -> Result<(), Box<dyn Error>> {
    const TEST: &str = "rust_runner_jit_directory_is_private";
    if !is_child_of(TEST) {
        rerun(TEST, "umask 0;", &[])?;
        return Ok(());
    }
    let fresh = Fixture::new(r#"[{"name":"velnor"}]"#)?;
    fresh.launcher().ensure_machine()?;
    assert_eq!(mode_of(&fresh.store.join("jit"))?, 0o700);
    assert!(fresh.store.join("work").is_dir());

    for mode in [0o755, 0o750, 0o710, 0o770, 0o701, 0o777] {
        let fixture = Fixture::new(r#"[{"name":"velnor"}]"#)?;
        std::fs::create_dir_all(fixture.store.join("jit"))?;
        std::fs::set_permissions(
            fixture.store.join("jit"),
            std::fs::Permissions::from_mode(mode),
        )?;
        assert_eq!(
            fixture.launcher().ensure_machine().err(),
            Some(HostError::RustRunner(
                RustRunnerFailure::JitDirectoryAccessible
            )),
            "{mode:o}"
        );
    }
    Ok(())
}

fn mode_of(path: &Path) -> Result<u32, Box<dyn Error>> {
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o7777)
}
