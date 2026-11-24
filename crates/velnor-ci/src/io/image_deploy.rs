// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{api_url, append_outputs, http, image_candidate, read_limited, token};
use crate::cli::ImageCommand;
use crate::cli_image::{ImageGateArgs, ImagePreflightArgs, ImageStageArgs, OpensslMode};
use crate::error::CiError;
use crate::github::Repo;
use crate::image_candidate::hashing::sha256_hex;
use crate::image_deploy::gate::{Wait, wait_for_required};
use crate::image_deploy::hub::{HUB_URL, preflight};
use crate::image_deploy::stage::{Glibc, Openssl, check};

pub mod sysroot;

const MAX_BINARY_BYTES: u64 = 1 << 30;

pub fn run(command: ImageCommand) -> Result<String, CiError> {
    match command {
        ImageCommand::VerifyCandidate(args) => image_candidate::verify_candidate(&args),
        ImageCommand::Build(args) => image_candidate::build(&args),
        ImageCommand::Publish(args) => image_candidate::publish::publish(&args),
        ImageCommand::Stage(args) => stage(&args),
        ImageCommand::Gate(args) => gate(&args),
        ImageCommand::Preflight(args) => hub_preflight(&args),
        ImageCommand::Sysroot(args) => sysroot::unpack(&args),
    }
}

fn read_binary(path: &Path) -> Result<Vec<u8>, String> {
    let real = fs::canonicalize(path).map_err(|_| "it was not built".to_owned())?;
    let file = fs::File::open(&real).map_err(|error| format!("it cannot be read: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("it cannot be read: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_BINARY_BYTES {
        return Err("it is not a regular file of a sane size".to_owned());
    }
    read_limited(file, MAX_BINARY_BYTES).map_err(|error| error.to_string())
}

fn staged_directory(to: &Path) -> Result<(), CiError> {
    match fs::read_dir(to) {
        Ok(entries) => {
            if entries.count() > 0 {
                return Err(CiError::new(format!(
                    "{} is not empty; the stage directory receives only the compiled binaries",
                    to.display()
                )));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(to)
            .map_err(|error| CiError::new(format!("cannot create {}: {error}", to.display()))),
        Err(error) => Err(CiError::new(format!(
            "cannot read {}: {error}",
            to.display()
        ))),
    }
}

pub fn stage(args: &ImageStageArgs) -> Result<String, CiError> {
    let ceiling = Glibc::parse(&args.glibc).ok_or_else(|| {
        CiError::new(format!("`{}` is not a glibc version like 2.41", args.glibc))
    })?;
    let openssl = match args.openssl {
        OpensslMode::None => Openssl::None,
        OpensslMode::Sysroot => Openssl::Sysroot,
        OpensslMode::Vendored => Openssl::Vendored,
    };
    let directory: PathBuf = args.target_dir.join(&args.target).join(&args.profile);
    let mut staged = Vec::new();
    let mut refusals = Vec::new();
    for bin in &args.bins {
        if bin.is_empty() || bin.contains(['/', '\\']) || bin.starts_with('.') {
            return Err(CiError::new(format!("`{bin}` is not a binary name")));
        }
        let path = directory.join(bin);
        match read_binary(&path)
            .map_err(|reason| vec![reason])
            .and_then(|bytes| check(&bytes, ceiling, openssl).map(|checked| (bytes, checked)))
        {
            Ok((bytes, checked)) => staged.push((bin, bytes, checked)),
            Err(reasons) => refusals.extend(
                reasons
                    .into_iter()
                    .map(|reason| format!("{} is refused: {reason}", path.display())),
            ),
        }
    }
    if !refusals.is_empty() {
        return Err(CiError::new(refusals.join("\n")));
    }
    staged_directory(&args.to)?;
    let mut report = Vec::new();
    for (bin, bytes, checked) in &staged {
        let target = args.to.join(bin);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o755)
            .open(&target)
            .and_then(|mut file| file.write_all(bytes))
            .map_err(|error| CiError::new(format!("cannot write {}: {error}", target.display())))?;
        report.push(format!(
            "{bin}: x86-64 dynamic, glibc ceiling {} (allowed {ceiling}), needs {}, {} bytes, sha256 {}",
            checked
                .ceiling
                .map_or_else(|| "none".to_owned(), |glibc| glibc.to_string()),
            checked.needed.join(" "),
            bytes.len(),
            sha256_hex(bytes)
        ));
    }
    Ok(report.join("\n") + "\n")
}

fn gate(args: &ImageGateArgs) -> Result<String, CiError> {
    let repo = Repo::parse(&args.repo)?;
    let github = http::HttpGitHub::new(api_url(), token()?, http::DEFAULT_TIMEOUT);
    let run_id = std::env::var("GITHUB_RUN_ID")
        .ok()
        .map(|id| id.parse::<u64>())
        .transpose()
        .map_err(|error| CiError::new(format!("invalid GITHUB_RUN_ID: {error}")))?;
    let outcome = wait_for_required(
        &github,
        &repo,
        &args.sha,
        args.require_tip,
        Wait {
            total: Duration::from_secs(args.wait_seconds),
            interval: Duration::from_secs(args.interval_seconds),
        },
        &std::thread::sleep,
        run_id,
    )?;
    append_outputs(&format!("ready={}\n", outcome.ready))?;
    Ok(outcome.report)
}

fn variable(name: &str) -> Result<String, CiError> {
    std::env::var(name).map_err(|_| CiError::new(format!("set {name}")))
}

fn hub_preflight(args: &ImagePreflightArgs) -> Result<String, CiError> {
    let outcome = preflight(
        &super::Https::new(),
        HUB_URL,
        &variable("VELNOR_HUB_USERNAME")?,
        &variable("VELNOR_HUB_TOKEN")?,
        &args.repository,
        &args.tag,
    )?;
    append_outputs(&format!("sha_exists={}\n", outcome.sha_exists))?;
    Ok(outcome.report)
}
