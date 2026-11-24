// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Places the mirror `--config` where Cargo keeps it next to the caller's own.
//!
//! Cargo reads `--config` as one group per command line position. A group before
//! the subcommand and a group after it do not merge: the one after the subcommand
//! replaces the other. A step such as `cargo doc --config 'build.rustdocflags=...'`
//! therefore dropped the mirror source replacement when the mirror came first. That
//! step resolved crates.io, unpacked the registry crates under another
//! `registry/src` directory, and the changed source path made Cargo rebuild every
//! dependency (`PathToSourceChanged`) each time the steps switched between the two.

use std::ffi::{OsStr, OsString};

const SUBCOMMANDS: &[&str] = &[
    "add",
    "bench",
    "build",
    "b",
    "check",
    "c",
    "clean",
    "doc",
    "d",
    "fetch",
    "fix",
    "info",
    "generate-lockfile",
    "init",
    "install",
    "locate-project",
    "metadata",
    "new",
    "owner",
    "package",
    "pkgid",
    "publish",
    "remove",
    "rm",
    "report",
    "run",
    "r",
    "rustc",
    "rustdoc",
    "search",
    "test",
    "t",
    "tree",
    "uninstall",
    "update",
    "vendor",
    "verify-project",
    "yank",
    "login",
    "logout",
    "help",
    "version",
    "config",
    "read-manifest",
    "git-checkout",
];
const VALUE_FLAGS: &[&str] = &["--config", "-Z", "-C"];

/// The arguments for the real Cargo: the mirror configuration first, so a later
/// configuration of the caller can still override it.
pub(super) fn with_mirror_config(config: &OsStr, args: Vec<OsString>) -> Vec<OsString> {
    let end = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    let regrouped = subcommand(&args[..end])
        .filter(|&at| args[at + 1..end].iter().any(is_config))
        .map(|at| regroup(config, &args, at));
    regrouped.unwrap_or_else(|| {
        ["--config".into(), config.to_owned()]
            .into_iter()
            .chain(args)
            .collect()
    })
}

fn subcommand(args: &[OsString]) -> Option<usize> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        let text = arg.to_str()?;
        if !text.starts_with('-') {
            return SUBCOMMANDS.contains(&text).then_some(at);
        }
        at += if VALUE_FLAGS.contains(&text) { 2 } else { 1 };
    }
    None
}

fn is_config(arg: &OsString) -> bool {
    arg.to_str()
        .is_some_and(|text| text == "--config" || text.starts_with("--config="))
}

fn regroup(config: &OsStr, args: &[OsString], at: usize) -> Vec<OsString> {
    let mut before = Vec::new();
    let mut moved = Vec::new();
    let mut rest = args[..at].iter();
    while let Some(arg) = rest.next() {
        if is_config(arg) {
            moved.push(arg.clone());
            if arg == "--config" {
                moved.extend(rest.next().cloned());
            }
        } else {
            before.push(arg.clone());
        }
    }
    before.push(args[at].clone());
    before.extend(["--config".into(), config.to_owned()]);
    before.extend(moved);
    before.extend_from_slice(&args[at + 1..]);
    before
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Vec<String> {
        let args = args.iter().map(OsString::from).collect();
        with_mirror_config(OsStr::new("mirror.toml"), args)
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn leads_when_the_caller_passes_no_configuration() {
        assert_eq!(
            run(&["doc", "--no-deps"]),
            ["--config", "mirror.toml", "doc", "--no-deps"]
        );
        assert_eq!(run(&[]), ["--config", "mirror.toml"]);
    }

    #[test]
    fn joins_the_group_after_the_subcommand() {
        assert_eq!(
            run(&["doc", "--config", "build.rustdocflags=[]", "--locked"]),
            [
                "doc",
                "--config",
                "mirror.toml",
                "--config",
                "build.rustdocflags=[]",
                "--locked"
            ]
        );
        assert_eq!(
            run(&["check", "--config=net.retry=0"]),
            ["check", "--config", "mirror.toml", "--config=net.retry=0"]
        );
    }

    #[test]
    fn keeps_the_order_of_global_then_command_configuration() {
        assert_eq!(
            run(&["--config", "a=1", "-Z", "x", "build", "--config", "b=2"]),
            [
                "-Z",
                "x",
                "build",
                "--config",
                "mirror.toml",
                "--config",
                "a=1",
                "--config",
                "b=2"
            ]
        );
    }

    #[test]
    fn leaves_global_configuration_and_program_arguments_alone() {
        assert_eq!(
            run(&["--config", "a=1", "build"]),
            ["--config", "mirror.toml", "--config", "a=1", "build"]
        );
        assert_eq!(
            run(&["run", "--", "--config", "x"]),
            ["--config", "mirror.toml", "run", "--", "--config", "x"]
        );
    }

    #[test]
    fn leaves_external_subcommands_to_their_own_parser() {
        assert_eq!(
            run(&["nextest", "run", "--config", "x"]),
            ["--config", "mirror.toml", "nextest", "run", "--config", "x"]
        );
    }
}
