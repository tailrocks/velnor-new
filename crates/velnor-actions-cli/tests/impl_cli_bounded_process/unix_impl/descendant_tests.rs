use super::{BoundedProcessError, run_bounded};
use std::ffi::OsStr;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn marker_path() -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("bounded-child-{}-{nonce}", std::process::id()))
}

fn marker_command(path: &OsStr, script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(script).env("MARKER", path);
    command
}

fn assert_marker_absent(path: &std::path::Path) {
    let appeared = path.exists();
    if appeared {
        std::fs::remove_file(path).expect("remove descendant marker");
    }
    assert!(!appeared, "descendant wrote its marker");
}

#[test]
fn timeout_kills_a_descendant_before_its_delayed_marker() {
    let marker = marker_path();
    let mut command = marker_command(
        marker.as_os_str(),
        "(sleep 0.4; printf marker > \"$MARKER\") & wait",
    );
    let result = run_bounded(&mut command, 8 * 1024, Duration::from_millis(120));
    assert!(matches!(result, Err(BoundedProcessError::TimedOut)));
    thread::sleep(Duration::from_millis(500));
    assert_marker_absent(&marker);
}

#[test]
fn successful_leader_exit_kills_a_descendant_holding_the_pipes() {
    let marker = marker_path();
    let mut command = marker_command(
        marker.as_os_str(),
        "(sleep 0.4; printf marker > \"$MARKER\") & exit 0",
    );
    let output = run_bounded(&mut command, 8 * 1024, Duration::from_secs(2))
        .expect("leader exits successfully and descendant is contained");
    assert!(output.success);
    assert_eq!(output.code, Some(0));
    assert_eq!(output.signal, None);
    thread::sleep(Duration::from_millis(500));
    assert_marker_absent(&marker);
}

#[test]
fn target_spawn_failure_still_cleans_the_owned_group_anchor() {
    let mut command = Command::new("/definitely/missing/bounded-target");
    let result = run_bounded(&mut command, 8 * 1024, Duration::from_secs(2));
    assert!(matches!(result, Err(BoundedProcessError::Io(_))));
}
