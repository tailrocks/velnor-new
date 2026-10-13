//! `status` and `doctor` follow the state directory.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn host() -> Command {
    Command::new(env!("CARGO_BIN_EXE_velnor-host"))
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-cli-ready-{label}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir(&path).map_err(|err| err.to_string())?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn cleanup(&self) -> Result<(), String> {
        match std::fs::remove_dir_all(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("scratch cleanup failed: {error}")),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "failed to remove CLI scratch {}: {error}",
                self.path.display()
            );
        }
    }
}

fn stdout(command: &mut Command) -> Result<String, String> {
    let output = command.output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(format!("status {:?}", output.status));
    }
    let text = String::from_utf8(output.stdout).map_err(|err| err.to_string())?;
    Ok(text.trim().to_owned())
}

#[test]
fn missing_state_is_waiting_for_credentials() -> Result<(), String> {
    let scratch = Scratch::new("missing")?;
    let path = scratch.path().join("state");
    let mut command = host();
    command.arg("--state").arg(&path).args(["status", "--json"]);
    let text = stdout(&mut command)?;
    if path.exists() {
        return Err("created".to_owned());
    }
    if text == r#"{"state":"waiting_for_credentials"}"# {
        scratch.cleanup()?;
        Ok(())
    } else {
        Err(text)
    }
}

#[test]
fn drain_flag_is_draining() -> Result<(), String> {
    let scratch = Scratch::new("drain")?;
    std::fs::write(scratch.path().join("drain"), b"1").map_err(|err| err.to_string())?;
    let mut command = host();
    command
        .arg("--state")
        .arg(scratch.path())
        .args(["status", "--json"]);
    let text = stdout(&mut command)?;
    if text == r#"{"state":"draining"}"# {
        scratch.cleanup()?;
        Ok(())
    } else {
        Err(text)
    }
}

#[test]
fn empty_directory_keeps_probe_off_the_state() -> Result<(), String> {
    let scratch = Scratch::new("doctor")?;
    let mut quiet = host();
    quiet.arg("--state").arg(scratch.path()).arg("doctor");
    let mut probed = host();
    probed
        .arg("--state")
        .arg(scratch.path())
        .args(["doctor", "--probe"]);
    let quiet = stdout(&mut quiet)?;
    let probed = stdout(&mut probed)?;
    if quiet != r#"{"state":"waiting_for_credentials","probe":false}"# {
        return Err(quiet);
    }
    if probed == r#"{"state":"waiting_for_credentials","probe":true}"# {
        scratch.cleanup()?;
        Ok(())
    } else {
        Err(probed)
    }
}

#[test]
fn malformed_config_is_degraded() -> Result<(), String> {
    let scratch = Scratch::new("toml")?;
    std::fs::write(scratch.path().join("host.toml"), b"not = toml")
        .map_err(|err| err.to_string())?;
    let mut command = host();
    command
        .arg("--state")
        .arg(scratch.path())
        .args(["status", "--json"]);
    let text = stdout(&mut command)?;
    if text == r#"{"state":"degraded"}"# {
        scratch.cleanup()?;
        Ok(())
    } else {
        Err(text)
    }
}

#[test]
fn oversized_config_is_degraded() -> Result<(), String> {
    let scratch = Scratch::new("oversized-config")?;
    let path = scratch.path().join("host.toml");
    std::fs::write(&path, vec![b' '; 65_537]).map_err(|err| err.to_string())?;
    let mut command = host();
    command
        .arg("--state")
        .arg(scratch.path())
        .args(["status", "--json"]);
    let text = stdout(&mut command)?;
    if text == r#"{"state":"degraded"}"# {
        scratch.cleanup()?;
        Ok(())
    } else {
        Err(text)
    }
}

#[cfg(unix)]
#[test]
fn fifo_config_is_rejected_without_blocking_status() -> Result<(), String> {
    let scratch = Scratch::new("fifo")?;
    let fifo = scratch.path().join("host.toml");
    let created = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .map_err(|error| error.to_string())?;
    if !created.success() {
        return Err(format!("mkfifo returned {created}"));
    }
    let mut command = host();
    command
        .arg("--state")
        .arg(scratch.path())
        .args(["status", "--json"]);
    let text = stdout(&mut command)?;
    if text == r#"{"state":"degraded"}"# {
        scratch.cleanup()?;
        Ok(())
    } else {
        Err(text)
    }
}
