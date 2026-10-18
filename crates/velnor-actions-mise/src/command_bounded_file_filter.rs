use super::{EnvPolicy, IsolatedCommand};
use std::ffi::{OsStr, OsString};
use std::process::Command;

pub(crate) fn command_for_file_filter(program: &OsStr, args: &[&str]) -> Command {
    let isolated = IsolatedCommand {
        program: program.to_os_string(),
        args: args.iter().map(OsString::from).collect(),
        cwd: None,
        extra_env: Vec::new(),
        policy: EnvPolicy::QualifiedProbe,
    };
    isolated.command()
}
