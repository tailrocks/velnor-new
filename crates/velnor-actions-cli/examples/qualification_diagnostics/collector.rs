// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn mutate_fixture(mode: &str) -> std::io::Result<()> {
    use std::io::Write;
    let is_generate = std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "generate");
    if (mode == "mutate-fixture-generate") != is_generate {
        return Ok(());
    }
    let root = super::executable::private_mutation_root()?;
    let current = std::env::current_dir()?.canonicalize()?;
    if !current.starts_with(&root) {
        return Err(std::io::Error::other("mutation outside private root"));
    }
    let path = if mode == "mutate-dogfood-plan" {
        if current.file_name().is_none_or(|name| name != "source") {
            return Ok(());
        }
        current.join(".github/CLAUDE.md")
    } else {
        if current.file_name().is_none_or(|name| name != "nested") {
            return Ok(());
        }
        current.join("Cargo.toml")
    };
    std::fs::OpenOptions::new()
        .append(true)
        .open(path)?
        .write_all(b"\n")
}

pub(super) fn is_plan_command() -> bool {
    use std::ffi::OsStr;

    let mut args = std::env::args_os();
    args.len() == 2 && args.nth(1).as_deref() == Some(OsStr::new("plan"))
}

pub(super) fn is_generate_command() -> bool {
    use std::ffi::OsStr;

    std::env::args_os().nth(1).as_deref() == Some(OsStr::new("generate"))
}
