use std::path::PathBuf;

use super::{
    ConfigReadError, DaemonIntent, daemon_backend_supported, daemon_intent, read_daemon_config_with,
};
use velnor_runner_host::{DaemonLock, HostConfig, HostError, HostPlatform};

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
fn daemon_reads_the_selected_config_file_not_state_host_toml() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("velnor-daemon-config-{}", std::process::id()));
    let state = dir.join("state");
    let selected = dir.join("operator-selected.toml");
    std::fs::create_dir_all(&state).map_err(|error| error.to_string())?;
    std::fs::write(state.join("host.toml"), b"not valid TOML\n")
        .map_err(|error| error.to_string())?;
    std::fs::write(&selected, SAMPLE).map_err(|error| error.to_string())?;

    let config = read_daemon_config_with(&selected, HostPlatform::Macos, |path, platform| {
        if platform != HostPlatform::Macos {
            return Err(());
        }
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(()),
        }
    })
    .map_err(|error| format!("selected config failed: {error:?}"))?
    .ok_or("selected config was treated as missing")?;
    if config.github.repository != "tailrocks/velnor-new" {
        return Err("daemon did not read the configured path".to_owned());
    }
    if !matches!(
        read_daemon_config_with(&state.join("host.toml"), HostPlatform::Macos, |path, _| {
            match std::fs::read_to_string(path) {
                Ok(text) => Ok(Some(text)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(()),
            }
        }),
        Err(ConfigReadError::Invalid)
    ) {
        return Err("fixture did not distinguish the legacy fallback path".to_owned());
    }
    std::fs::remove_dir_all(dir).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn daemon_backend_does_not_fall_back_to_the_legacy_launcher_on_linux() {
    assert!(!daemon_backend_supported(HostPlatform::Linux));
    assert!(daemon_backend_supported(HostPlatform::Macos));
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

mod daemon_linux_tests;
