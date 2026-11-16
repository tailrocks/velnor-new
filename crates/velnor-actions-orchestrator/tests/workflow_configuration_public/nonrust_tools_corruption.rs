// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    io::Write as _,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use super::{
    nonrust_tools_fixture::{Archive, Fixture, copy_tree},
    nonrust_tools_roundtrip::{RenderedCommand, execute},
    tools_cache_verify_hook::Sandbox,
};

pub(super) fn verify(
    fixture: &Fixture,
    source: &Sandbox,
    step: &RenderedCommand,
    archives: &[&Archive],
) -> Result<(), Box<dyn Error>> {
    fixture.allow_archives()?;
    for archive in archives {
        for corruption in [
            "remove",
            "move",
            "mode",
            "archive",
            "missing-archive",
            "pin",
            "platform",
            "version",
        ] {
            check(fixture, source, step, archive, corruption)?;
        }
    }
    Ok(())
}

fn check(
    fixture: &Fixture,
    source: &Sandbox,
    step: &RenderedCommand,
    archive: &Archive,
    corruption: &str,
) -> Result<(), Box<dyn Error>> {
    let runner = Sandbox::new()?;
    let installs = runner.mise_root().join("installs");
    copy_tree(
        &source.mise_root().join("bin"),
        &runner.mise_root().join("bin"),
    )?;
    copy_tree(
        &source.mise_root().join("installs").join(&archive.tool),
        &installs.join(&archive.tool),
    )?;
    let saved = installs
        .join(".velnor-verified-archives")
        .join(&archive.sha256);
    fs::create_dir_all(saved.parent().ok_or("archive has no parent")?)?;
    fs::copy(
        source
            .mise_root()
            .join("installs/.velnor-verified-archives")
            .join(&archive.sha256),
        &saved,
    )?;
    let version = installs.join(&archive.tool).join(&archive.version);
    let binary =
        find_binary(&version, &archive.tool)?.ok_or("real installed executable missing")?;
    let mut command = RenderedCommand {
        run: step.run.clone(),
        shell: step.shell.clone(),
        env: step.env.clone(),
    };
    command
        .env
        .insert("HTTPS_PROXY".to_owned(), "http://127.0.0.1:1".to_owned());
    command.env.insert("NO_PROXY".to_owned(), String::new());
    match corruption {
        "remove" => fs::remove_file(&binary)?,
        "move" => fs::rename(&binary, binary.with_file_name("moved-executable"))?,
        "mode" => fs::set_permissions(&binary, fs::Permissions::from_mode(0o644))?,
        "archive" => fs::OpenOptions::new()
            .append(true)
            .open(&saved)?
            .write_all(&[0; 4])?,
        "missing-archive" => fs::remove_file(&saved)?,
        "pin" => {
            let pins = command
                .env
                .get_mut("VELNOR_TOOL_PINS")
                .ok_or("no generated pins")?;
            *pins = pins
                .lines()
                .filter(|row| row.split_whitespace().next() != Some(&archive.tool))
                .collect::<Vec<_>>()
                .join("\n");
        }
        "platform" => {
            command
                .env
                .insert("RUNNER_ARCH".to_owned(), "unsupported".to_owned());
        }
        "version" => fs::rename(&version, installs.join(&archive.tool).join("0.0.0"))?,
        _ => return Err("unknown corruption scenario".into()),
    }
    execute(fixture, &runner, &command)?;
    assert!(
        !version.exists(),
        "{} {corruption} was accepted",
        archive.tool
    );
    assert!(!installs.join(&archive.tool).join("0.0.0").exists());
    fixture.install(&runner, &archive.tool, &archive.version)?;
    fixture.execute(&runner, &archive.tool, &archive.version)?;
    eprintln!(
        "{} corruption={corruption} rejected=true cold_install=true",
        archive.tool
    );
    Ok(())
}

fn find_binary(root: &Path, tool: &str) -> Result<Option<PathBuf>, Box<dyn Error>> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_file() && entry.file_name() == tool {
            return Ok(Some(entry.path()));
        }
        if kind.is_dir()
            && let Some(path) = find_binary(&entry.path(), tool)?
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}
