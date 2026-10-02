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
