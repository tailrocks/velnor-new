use std::path::PathBuf;

use super::{DaemonIntent, daemon_intent};
use velnor_runner_host::{DaemonLock, HostConfig, HostError};

const SAMPLE: &str = concat!(
    "schema = 1\n",
    "[github]\n",
    "repository = \"tailrocks/velnor-new\"\n",
    "scale_set_name = \"ubuntu-26.04-scale-set\"\n",
    "credential_ref = \"keychain:com.tailrocks.velnor.host/local\"\n",
    "[host]\n",
    "max_jobs = 1\n",
    "[docker]\n",
    "context = \"orbstack\"\n",
    "platform = \"linux/amd64\"\n",
    "endpoint = \"unix:///var/run/docker.sock\"\n",
);

#[test]
fn credential_text_trims_and_rejects_empty() {
    assert_eq!(
        super::credential_text(b"canary-token\n"),
        Some("canary-token")
    );
    assert_eq!(super::credential_text(b"\n"), None);
    assert_eq!(super::credential_text(&[0xff, 0xfe]), None);
}

#[test]
fn missing_config_waits() {
    assert_eq!(daemon_intent(None), DaemonIntent::Wait);
}

#[test]
fn valid_host_toml_listens() {
    assert_eq!(daemon_intent(Some(SAMPLE)), DaemonIntent::Listen);
}

#[test]
fn bad_toml_is_err_and_hides_the_secret() {
    let bad = SAMPLE.replace("schema = 1", "schema = 2\ntoken = \"secret\"");
    assert_eq!(daemon_intent(Some(&bad)), DaemonIntent::Err);
    let error = match HostConfig::parse(&bad) {
        Ok(_) => HostError::Config,
        Err(error) => error,
    };
    let text = format!("{error}");
    assert_eq!(text, "invalid config");
    assert!(!text.contains("secret"));
}

#[test]
fn second_daemon_lock_fails_closed() -> Result<(), String> {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("velnor-daemon-lock-{}", std::process::id()));
    remove_tree(&dir);
    std::fs::create_dir_all(&dir).map_err(|_| "mkdir".to_owned())?;
    let path = dir.join("daemon.lock");
    let first = DaemonLock::try_acquire(&path).map_err(|_| "first".to_owned())?;
    if !first.is_held() {
        return Err("not held".to_owned());
    }
    match DaemonLock::try_acquire(&path) {
        Err(HostError::Lock) => {}
        Ok(_) => return Err("second acquired".to_owned()),
        Err(_) => return Err("other".to_owned()),
    }
    drop(first);
    remove_tree(&dir);
    Ok(())
}

fn remove_tree(dir: &std::path::Path) {
    match std::fs::remove_dir_all(dir) {
        Ok(()) | Err(_) => {}
    }
}
