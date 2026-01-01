//! Help text lists every operator command.

use clap::CommandFactory;
use velnor_runner_cli::Cli;

#[test]
fn help_lists_every_command() -> Result<(), String> {
    let mut command = Cli::command();
    let mut buffer = Vec::new();
    command
        .write_long_help(&mut buffer)
        .map_err(|err| err.to_string())?;
    let text = String::from_utf8(buffer).map_err(|err| err.to_string())?;
    for name in [
        "connect",
        "status",
        "doctor",
        "logs",
        "drain",
        "resume",
        "service",
        "daemon",
        "compare",
        "disconnect",
    ] {
        assert!(text.contains(name), "{name} missing from {text}");
    }
    Ok(())
}

#[test]
fn help_exits_success_and_a_bad_command_does_not() -> Result<(), String> {
    let help = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-host"))
        .arg("--help")
        .output()
        .map_err(|err| err.to_string())?;
    if !help.status.success() {
        return Err(format!("help status {:?}", help.status));
    }
    let bad = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-host"))
        .arg("no-such-command")
        .output()
        .map_err(|err| err.to_string())?;
    if bad.status.success() {
        return Err("unknown command succeeded".to_owned());
    }
    Ok(())
}
