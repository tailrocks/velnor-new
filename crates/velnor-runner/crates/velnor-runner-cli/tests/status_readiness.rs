//! `status` and `doctor` follow the state directory.

use std::path::{Path, PathBuf};
use std::process::Command;

fn host() -> Command {
    Command::new(env!("CARGO_BIN_EXE_velnor-host"))
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Result<Self, String> {
        let path =
            std::env::temp_dir().join(format!("velnor-cli-ready-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|err| err.to_string())?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let removed = std::fs::remove_dir_all(&self.path);
        let _kept = removed.err().map(|err| err.kind());
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
    let path = std::env::temp_dir().join(format!("velnor-cli-miss-{}", std::process::id()));
    let _removed = std::fs::remove_dir_all(&path);
    let mut command = host();
    command.arg("--state").arg(&path).args(["status", "--json"]);
    let text = stdout(&mut command)?;
    if path.exists() {
        return Err("created".to_owned());
    }
    if text == r#"{"state":"waiting_for_credentials"}"# {
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
        Ok(())
    } else {
        Err(probed)
    }
}

#[test]
fn invalid_config_stays_waiting_for_credentials() -> Result<(), String> {
    let scratch = Scratch::new("toml")?;
    std::fs::write(scratch.path().join("host.toml"), b"not = toml")
        .map_err(|err| err.to_string())?;
    let mut command = host();
    command
        .arg("--state")
        .arg(scratch.path())
        .args(["status", "--json"]);
    let text = stdout(&mut command)?;
    if text == r#"{"state":"waiting_for_credentials"}"# {
        Ok(())
    } else {
        Err(text)
    }
}
