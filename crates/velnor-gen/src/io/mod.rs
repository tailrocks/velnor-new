// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::Command as Process;

pub use std::process::ExitCode as Exit;

use clap::{CommandFactory, FromArgMatches};
use velnor_model::{RepoPath, RepoView, SAMPLES};
use velnor_rust::METADATA_PATH;

mod check;
mod error;
mod identity;
mod pin;
pub use identity::repository_name;
mod read;
mod recorded;
mod ruleset;
mod sample;
mod tracked;
mod workflows;

use crate::cli::{Cli, Command, ReleaseCommand};
use crate::{Drift, check_tag, notes, plan_generate};
pub use check::check_metadata;
pub use error::Error;
use pin::pin_helper;
pub use pin::{lock_root, write_lock};
pub use read::{add_manifests, read_files, view_of};
use read::{read_tracked, read_view, regular_file};
pub use recorded::{read_recorded_checkout, write_metadata};
pub use sample::{
    Recording, SAMPLE_REPOSITORY, SampleWriteOutcome, record_sample, recorded_files, recording_of,
    relocate_metadata, write_sample,
};
use tracked::tracked_sizes;
use workflows::{ENGINE_CONFIG, workflow_paths};

#[must_use]
pub fn version_text() -> String {
    format!(
        "{} ({})",
        env!("CARGO_PKG_VERSION"),
        option_env!("VELNOR_COMMIT").unwrap_or("unknown")
    )
}

pub fn parse_cli<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let matches = Cli::command()
        .version(version_text())
        .try_get_matches_from(args)?;
    Cli::from_arg_matches(&matches)
}

fn tag_verdict(tag: &str) -> Exit {
    match check_tag(env!("CARGO_PKG_VERSION"), tag) {
        Ok(message) => {
            println!("{message}");
            Exit::SUCCESS
        }
        Err(message) => {
            eprintln!("velnor-gen: {message}");
            Exit::from(1)
        }
    }
}

fn finish(result: Result<String, Error>) -> Exit {
    match result {
        Ok(message) => {
            println!("{message}");
            Exit::SUCCESS
        }
        Err(error) => {
            eprintln!("velnor-gen: {error}");
            Exit::from(2)
        }
    }
}

#[must_use]
pub fn main() -> Exit {
    let cli = match parse_cli(std::env::args_os()) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    match &cli.command {
        Command::RecordSample { checkout, name } => {
            return finish(
                record_sample(&cli.root, checkout, name).map(|outcome| match outcome {
                    SampleWriteOutcome::Recorded => format!("recorded {SAMPLES}/{name}"),
                    SampleWriteOutcome::RecordedWithCleanupFailure {
                        staging_path,
                        error,
                    } => format!(
                        "recorded {SAMPLES}/{name}; cleanup failed for {}: {error}",
                        staging_path.display()
                    ),
                }),
            );
        }
        Command::PinHelper { version } => return finish(pin_helper(&cli.root, version)),
        Command::CheckTag { tag } => return tag_verdict(tag),
        Command::Release(ReleaseCommand::Ruleset) => return ruleset::print(&cli.root),
        Command::Release(ReleaseCommand::RulesetDrift { live }) => {
            return ruleset::drift(&cli.root, live);
        }
        Command::SuggestGroups => return suggest(&cli.root),
        Command::Generate | Command::Check => {}
    }
    match execute(&cli) {
        Ok(drifts) if cli.command == Command::Check && !drifts.is_empty() => {
            for drift in &drifts {
                eprintln!("{drift}");
            }
            eprintln!("run `velnor-gen generate` to update the generated files");
            Exit::from(1)
        }
        Ok(drifts) => {
            if drifts.is_empty() {
                println!("generated files are up to date");
            }
            for drift in &drifts {
                match drift {
                    Drift::Changed(path) | Drift::Missing(path) => println!("wrote {path}"),
                    Drift::Stale(path) => println!("removed {path}"),
                    Drift::Foreign(path) | Drift::Unmarked(path) => println!("kept {path}"),
                }
            }
            Exit::SUCCESS
        }
        Err(error) => {
            eprintln!("velnor-gen: {error}");
            Exit::from(2)
        }
    }
}

fn suggest(path: &Path) -> Exit {
    let result = toplevel(path)
        .and_then(|root| read_view(&root, false))
        .and_then(|(view, _)| crate::suggest_groups(&view).map_err(Error::Generate));
    match result {
        Ok((text, notes)) => {
            for note in notes {
                eprintln!("velnor-gen: {note}");
            }
            if text.is_empty() {
                eprintln!(
                    "velnor-gen: no test binary takes more than a third of its root's test time"
                );
            } else {
                print!("{text}");
            }
            Exit::SUCCESS
        }
        Err(error) => {
            eprintln!("velnor-gen: {error}");
            Exit::from(2)
        }
    }
}

fn execute(cli: &Cli) -> Result<Vec<Drift>, Error> {
    let root = toplevel(&cli.root)?;
    let mut wanted = workflow_paths(&root)?;
    wanted.push(METADATA_PATH);
    let workflows = read_files(&root, &wanted)?;
    let engine_configured = regular_file(&root, &ENGINE_CONFIG)?.is_some();
    let view = read_view(&root, false)?.0;
    let (expected, drifts) =
        plan_generate(&view, &workflows, engine_configured, &mut std::io::stderr())
            .map_err(Error::Generate)?;
    if cli.command == Command::Generate {
        for note in notes(&view).map_err(Error::Generate)? {
            eprintln!("velnor-gen: note: {note}");
        }
        if let Some(note) =
            crate::sparse_note(&view, &tracked_sizes(&root, &view)).map_err(Error::Generate)?
        {
            eprintln!("velnor-gen: note: {note}");
        }
    }
    if let Some(Drift::Foreign(path)) = drifts.iter().find(|d| matches!(d, Drift::Foreign(_))) {
        return Err(Error::Foreign(path.clone()));
    }
    if cli.command == Command::Generate {
        apply(&root, &expected, &drifts)?;
    }
    Ok(drifts)
}

pub fn apply(
    root: &Path,
    expected: &BTreeMap<RepoPath, String>,
    drifts: &[Drift],
) -> Result<(), Error> {
    for change in drifts {
        let path = change.path();
        let existing = regular_file(root, path)?;
        match (change, expected.get(path), existing) {
            (Drift::Foreign(_), _, _) => return Err(Error::Foreign(path.clone())),
            (Drift::Unmarked(_), _, _) | (_, None, None) => {}
            (_, Some(content), _) => replace(&root.join(path.as_str()), content.as_bytes())?,
            (_, None, Some(file)) => {
                fs::remove_file(&file).map_err(|e| Error::File(file.clone(), e))?;
            }
        }
    }
    Ok(())
}

fn replace(file: &Path, content: &[u8]) -> Result<(), Error> {
    let parent = file
        .parent()
        .ok_or_else(|| Error::File(file.into(), ErrorKind::InvalidInput.into()))?;
    fs::create_dir_all(parent).map_err(|e| Error::File(parent.into(), e))?;
    let mut name = OsString::from(".");
    name.push(file.file_name().unwrap_or_default());
    name.push(".velnor-gen");
    let temporary = parent.join(name);
    match fs::remove_file(&temporary) {
        Err(error) if error.kind() != ErrorKind::NotFound => {
            return Err(Error::File(temporary, error));
        }
        _ => {}
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut handle| handle.write_all(content))
        .and_then(|()| fs::rename(&temporary, file))
        .map_err(|e| Error::File(file.into(), e))
}

pub fn manifest_file(root: &Path, manifest: &RepoPath) -> Result<PathBuf, Error> {
    regular_file(
        root,
        &manifest.sibling(&RepoPath::from_static("Cargo.lock")),
    )?;
    regular_file(root, manifest)?.ok_or_else(|| Error::NotRegular(manifest.clone()))
}

fn toplevel(path: &Path) -> Result<PathBuf, Error> {
    let output = program(
        path,
        "git",
        &[OsStr::new("rev-parse"), OsStr::new("--show-toplevel")],
    )?;
    let text = String::from_utf8(output)
        .map_err(|_| Error::Repository("the repository path is not UTF-8".to_owned()))?;
    let root = PathBuf::from(text.trim_end_matches('\n'));
    fs::canonicalize(&root).map_err(|e| Error::File(root, e))
}

#[derive(Debug)]
pub struct Checkout {
    pub root: PathBuf,
    pub view: RepoView,
}

pub fn read_checkout(path: &Path) -> Result<Checkout, Error> {
    let root = toplevel(path)?;
    let view = read_view(&root, false)?.0;
    Ok(Checkout { root, view })
}

fn program(directory: &Path, name: &str, args: &[&OsStr]) -> Result<Vec<u8>, Error> {
    let label = format!("{name} {}", args[0].display());
    let output = Process::new(name)
        .args(args)
        .current_dir(directory)
        .output()
        .map_err(|error| Error::Program(label.clone(), error.to_string()))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(Error::Program(
            label,
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}
