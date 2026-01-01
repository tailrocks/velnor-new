use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use velnor_runner_github::{BearerRole, Method, RequestPurpose, SessionRequest, TransportFail};

use super::{perform_curl, perform_curl_cancellable, perform_curl_until_cancellable};

#[test]
fn expired_absolute_deadline_does_not_spawn_curl() {
    let directory = TestDirectory::new();
    let executable = directory.path.join("curl-stub");
    let marker = directory.path.join("spawned");
    let script = format!("#!/bin/sh\n: > '{}'\n", marker.display());
    fs::write(&executable, script).expect("stub script should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .expect("stub should be executable");
    let expired_at = Instant::now();
    let request = SessionRequest {
        purpose: RequestPurpose::RepositoryRead,
        bearer_role: BearerRole::GithubRestCredential,
        method: Method::Get,
        path: "repos/acme/widget".to_owned(),
        query: None,
        headers: Vec::new(),
        body: Vec::new(),
    };

    assert!(matches!(
        perform_curl_until_cancellable(
            executable.to_str().expect("stub path is UTF-8"),
            "https://api.github.com/repos/acme/widget",
            &request,
            1024,
            expired_at,
            &AtomicBool::new(false),
        ),
        Err(TransportFail::Timeout)
    ));
    assert!(!marker.exists(), "expired work must not spawn curl");
}

#[test]
fn control_characters_are_rejected_before_curl_is_spawned() {
    let directory = TestDirectory::new();
    let executable = directory.path.join("curl-stub");
    let marker = directory.path.join("spawned");
    let script = format!("#!/bin/sh\n: > '{}'\n", marker.display());
    fs::write(&executable, script).expect("stub script should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .expect("stub should be executable");

    let requests = [
        SessionRequest {
            purpose: RequestPurpose::RepositoryRead,
            bearer_role: BearerRole::GithubRestCredential,
            method: Method::Get,
            path: "repos/acme/widget".to_owned(),
            query: None,
            headers: Vec::new(),
            body: Vec::new(),
        },
        SessionRequest {
            purpose: RequestPurpose::RepositoryRead,
            bearer_role: BearerRole::GithubRestCredential,
            method: Method::Get,
            path: "repos/acme/widget".to_owned(),
            query: None,
            headers: vec![("X-Test".to_owned(), "bad\nvalue".to_owned())],
            body: Vec::new(),
        },
        SessionRequest {
            purpose: RequestPurpose::RegistrationTokenIssue,
            bearer_role: BearerRole::GithubRestCredential,
            method: Method::Post,
            path: "repos/acme/widget/actions/runners/registration-token".to_owned(),
            query: None,
            headers: Vec::new(),
            body: b"bad\tvalue".to_vec(),
        },
    ];
    let urls = [
        "https://api.github.com/repos/acme/widget\n--next",
        "https://api.github.com/repos/acme/widget",
        "https://api.github.com/repos/acme/widget/actions/runners/registration-token",
    ];
    for (request, url) in requests.iter().zip(urls) {
        assert!(matches!(
            perform_curl(
                executable.to_str().expect("stub path is UTF-8"),
                url,
                request,
                1024,
                Duration::from_secs(1),
            ),
            Err(TransportFail::Reset)
        ));
        assert!(!marker.exists(), "invalid config must fail before spawn");
    }
}

#[test]
fn cancellation_kills_and_reaps_the_owned_curl_process() {
    let directory = TestDirectory::new();
    let executable = directory.path.join("curl-stub");
    let pid_file = directory.path.join("pid");
    let script = format!(
        "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\ncat >/dev/null\nexec sleep 30\n",
        pid_file.display()
    );
    fs::write(&executable, script).expect("stub script should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .expect("stub should be executable");
    let request = SessionRequest {
        purpose: RequestPurpose::ActionsAdminExchange,
        bearer_role: BearerRole::RegistrationToken,
        method: Method::Post,
        path: "actions/runner-registration".to_owned(),
        query: None,
        headers: Vec::new(),
        body: b"bounded body".to_vec(),
    };
    let cancellation = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancellation);
    let executable_text = executable.to_string_lossy().into_owned();
    let worker = thread::spawn(move || {
        perform_curl_cancellable(
            &executable_text,
            "https://pipelinesghubeus13.actions.githubusercontent.com/actions/runner-registration",
            &request,
            1024,
            Duration::from_secs(5),
            &worker_cancel,
        )
    });

    let started = Instant::now();
    while !pid_file.exists() && started.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(5));
    }
    if !pid_file.exists() {
        cancellation.store(true, Ordering::Release);
        let _result = worker.join();
        panic!("owned curl stub did not start");
    }
    thread::sleep(Duration::from_millis(50));
    let pid = fs::read_to_string(&pid_file).expect("stub PID should be recorded");
    cancellation.store(true, Ordering::Release);
    assert!(matches!(
        worker.join().expect("worker should return"),
        Err(TransportFail::Reset)
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(
        !Command::new("kill")
            .args(["-0", pid.trim()])
            .status()
            .expect("process existence probe should run")
            .success(),
        "canceled curl PID must be reaped before the worker returns"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn cancellation_returns_bounded_when_an_inherited_pipe_stays_open() {
    let directory = TestDirectory::new();
    let executable = directory.path.join("curl-stub");
    let descendant_file = directory.path.join("descendant-pid");
    let script = format!(
        "#!/bin/sh\nsetsid sh -c 'sleep 1' &\nprintf '%s' \"$!\" > '{}'\nexit 0\n",
        descendant_file.display()
    );
    fs::write(&executable, script).expect("stub script should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .expect("stub should be executable");
    let request = SessionRequest {
        purpose: RequestPurpose::ActionsAdminExchange,
        bearer_role: BearerRole::RegistrationToken,
        method: Method::Post,
        path: "actions/runner-registration".to_owned(),
        query: None,
        headers: Vec::new(),
        body: b"bounded body".to_vec(),
    };
    let executable_text = executable.to_string_lossy().into_owned();
    let cancellation = Arc::new(AtomicBool::new(false));
    let worker_cancellation = Arc::clone(&cancellation);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let result = perform_curl_cancellable(
            &executable_text,
            "https://pipelinesghubeus13.actions.githubusercontent.com/actions/runner-registration",
            &request,
            1024,
            Duration::from_secs(3),
            &worker_cancellation,
        );
        let _sent = sender.send(result.map(|_| ()).map_err(|_| ()));
    });
    let started = Instant::now();
    while !descendant_file.exists() && started.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        descendant_file.exists(),
        "detached pipe holder should start"
    );
    let pid = fs::read_to_string(&descendant_file).expect("descendant PID should be recorded");
    cancellation.store(true, Ordering::Release);
    let result = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("cleanup must not join a pipe reader past its deadline");
    assert_eq!(result, Err(()));
    assert!(started.elapsed() < Duration::from_secs(1));
    worker
        .join()
        .expect("bounded transport worker should finish");
    wait_until_process_exits(pid.trim(), Duration::from_secs(2));
}

#[cfg(target_os = "linux")]
#[test]
fn escaped_pipe_holder_survives_after_curl_leader_reap() {
    let directory = TestDirectory::new();
    let executable = directory.path.join("curl-stub");
    let leader_file = directory.path.join("leader-pid");
    let descendant_file = directory.path.join("descendant-pid");
    let script = format!(
        "#!/bin/sh\nsetsid sh -c 'sleep 3' &\nprintf '%s' \"$$\" > '{}'\nprintf '%s' \"$!\" > '{}'\nsleep 0.1\nexit 0\n",
        leader_file.display(),
        descendant_file.display()
    );
    fs::write(&executable, script).expect("stub script should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .expect("stub should be executable");
    let request = SessionRequest {
        purpose: RequestPurpose::RepositoryRead,
        bearer_role: BearerRole::GithubRestCredential,
        method: Method::Get,
        path: "repos/acme/widget".to_owned(),
        query: None,
        headers: Vec::new(),
        body: Vec::new(),
    };
    let executable_text = executable.to_string_lossy().into_owned();
    let cancellation = Arc::new(AtomicBool::new(false));
    let worker_cancellation = Arc::clone(&cancellation);
    let worker = thread::spawn(move || {
        perform_curl_cancellable(
            &executable_text,
            "https://api.github.com/repos/acme/widget",
            &request,
            1024,
            Duration::from_secs(4),
            &worker_cancellation,
        )
    });
    let started = Instant::now();
    while !leader_file.exists() && started.elapsed() < Duration::from_secs(1) {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(leader_file.exists(), "curl leader should start");
    while !descendant_file.exists() && started.elapsed() < Duration::from_secs(1) {
        thread::sleep(Duration::from_millis(5));
    }
    let leader = fs::read_to_string(&leader_file).expect("leader PID should be recorded");
    let descendant =
        fs::read_to_string(&descendant_file).expect("detached pipe holder PID should be recorded");
    wait_until_process_reaped(leader.trim(), Duration::from_secs(2));
    assert_process_is_live(descendant.trim());
    cancellation.store(true, Ordering::Release);
    let result = worker.join().expect("bounded worker should return");
    assert!(matches!(result, Err(TransportFail::Reset)));
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_process_is_live(descendant.trim());
    wait_until_process_exits(descendant.trim(), Duration::from_secs(3));
}

#[cfg(target_os = "linux")]
fn wait_until_process_exits(pid: &str, timeout: Duration) {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !process_is_live(pid) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("detached pipe holder did not exit before cleanup test deadline");
}

#[cfg(target_os = "linux")]
fn wait_until_process_reaped(pid: &str, timeout: Duration) {
    let started = Instant::now();
    let state_path = PathBuf::from(format!("/proc/{pid}/stat"));
    while state_path.exists() && started.elapsed() < timeout {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(!state_path.exists(), "curl leader should be reaped");
}

#[cfg(target_os = "linux")]
fn assert_process_is_live(pid: &str) {
    assert!(
        process_is_live(pid),
        "detached pipe holder must survive after curl leader is reaped"
    );
}

#[cfg(target_os = "linux")]
fn process_is_live(pid: &str) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.split_whitespace()
            .nth(2)
            .is_some_and(|state| state != "Z" && state != "X")
    })
}

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("velnor-curl-{}-{timestamp}", std::process::id()));
        fs::create_dir(&path).expect("test directory should be created");
        Self { path }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.path);
    }
}
