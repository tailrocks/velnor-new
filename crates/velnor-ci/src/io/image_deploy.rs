// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{api_url, append_outputs, http, image_candidate, read_limited, token};
use crate::cli::ImageCommand;
use crate::cli_image::{
    GateOnly, ImageAdvanceArgs, ImageAuthorizeArgs, ImageChangesArgs, ImageGateArgs,
    ImagePreflightArgs, ImagePromoteArgs, ImageStageArgs, OpensslMode,
};
use crate::error::CiError;
use crate::github::Repo;
use crate::image_candidate::hashing::sha256_hex;
use crate::image_deploy::advance::{Advancement, advance};
use crate::image_deploy::authority::{Release, authorize};
use crate::image_deploy::changes::{parse_matches, select};
use crate::image_deploy::gate::{
    Clock, Request, Wait, exemptions_only, tip_only, wait_for_required,
};
use crate::image_deploy::hub::{HUB_URL, preflight};
use crate::image_deploy::promote::{Promotion, live_tag_commit, promote};
use crate::image_deploy::stage::{Glibc, Openssl, check};

mod registry;
pub mod sysroot;

const MAX_BINARY_BYTES: u64 = 1 << 30;

pub fn run(command: ImageCommand) -> Result<String, CiError> {
    match command {
        ImageCommand::VerifyCandidate(args) => image_candidate::verify_candidate(&args),
        ImageCommand::Build(args) => image_candidate::build(&args),
        ImageCommand::Publish(args) => image_candidate::publish::publish(&args),
        ImageCommand::Stage(args) => stage_command(&args),
        ImageCommand::Gate(args) => gate(&args),
        ImageCommand::Changes(args) => changes(&args),
        ImageCommand::Advance(args) => advance_branch(&args),
        ImageCommand::Preflight(args) => hub_preflight(&args),
        ImageCommand::Authorize(args) => authorize_release(&args),
        ImageCommand::Promote(args) => promote_release(&args),
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

fn no_link_on(to: &Path, within: &Path) -> Result<(), CiError> {
    let below = to.strip_prefix(within).unwrap_or_else(|_| Path::new(""));
    let mut so_far = within.to_path_buf();
    let inspect = |path: &Path| match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(CiError::new(format!(
            "{} is a symbolic link, and the stage directory and its parents must be real directories",
            path.display()
        ))),
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CiError::new(format!(
            "cannot inspect {}: {error}",
            path.display()
        ))),
    };
    for part in below.components() {
        so_far.push(part);
        if !inspect(&so_far)? {
            return Ok(());
        }
    }
    inspect(to).map(|_| ())
}

fn staged_directory(to: &Path, within: &Path) -> Result<(), CiError> {
    no_link_on(to, within)?;
    match fs::read_dir(to) {
        Ok(entries) => {
            if entries.count() > 0 {
                return Err(CiError::new(format!(
                    "{} is not empty; the stage directory receives only the compiled binaries",
                    to.display()
                )));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(to).map_err(|error| {
                CiError::new(format!("cannot create {}: {error}", to.display()))
            })?;
        }
        Err(error) => {
            return Err(CiError::new(format!(
                "cannot read {}: {error}",
                to.display()
            )));
        }
    }
    no_link_on(to, within)
}

#[derive(Debug)]
pub struct Staged {
    pub report: String,
    pub directory: PathBuf,
}

fn stage_command(args: &ImageStageArgs) -> Result<String, CiError> {
    let staged = stage_to(args)?;
    if args.private_under.is_some() {
        append_outputs(&format!("dir={}\n", staged.directory.display()))?;
    }
    Ok(staged.report)
}

pub fn stage(args: &ImageStageArgs) -> Result<String, CiError> {
    stage_to(args).map(|staged| staged.report)
}

fn private_directory(under: &Path) -> Result<PathBuf, CiError> {
    let mut random = [0_u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut source| std::io::Read::read_exact(&mut source, &mut random))
        .map_err(|error| CiError::new(format!("cannot read random bytes: {error}")))?;
    let mut name = String::new();
    for byte in random {
        let _ = write!(name, "{byte:02x}");
    }
    let directory = under.join(format!("image-stage-{name}"));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(|error| CiError::new(format!("cannot create {}: {error}", directory.display())))?;
    Ok(directory)
}

pub fn stage_to(args: &ImageStageArgs) -> Result<Staged, CiError> {
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
    for (index, bin) in args.bins.iter().enumerate() {
        if args.bins[..index].contains(bin) {
            return Err(CiError::new(format!("--bin {bin} is given twice")));
        }
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
    let within = match &args.within {
        Some(within) => within.clone(),
        None => std::env::current_dir()
            .map_err(|error| CiError::new(format!("cannot read the working directory: {error}")))?,
    };
    let to = if let Some(under) = &args.private_under {
        no_link_on(under, under)?;
        private_directory(under)?
    } else if args.to.is_absolute() {
        args.to.clone()
    } else {
        within.join(&args.to)
    };
    if args.private_under.is_none() {
        staged_directory(&to, &within)?;
    }
    let mode = if args.private_under.is_some() {
        0o555
    } else {
        0o755
    };
    let mut report = Vec::new();
    for (bin, bytes, checked) in &staged {
        let target = to.join(bin);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&target)
            .and_then(|mut file| {
                file.write_all(bytes)?;
                file.set_permissions(fs::Permissions::from_mode(mode))
            })
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
    only_staged_files(
        &to,
        &staged
            .iter()
            .map(|(bin, _, _)| (*bin).as_str())
            .collect::<Vec<_>>(),
    )?;
    Ok(Staged {
        report: report.join("\n") + "\n",
        directory: to,
    })
}

fn only_staged_files(to: &Path, names: &[&str]) -> Result<(), CiError> {
    let entries = fs::read_dir(to)
        .map_err(|error| CiError::new(format!("cannot read {}: {error}", to.display())))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| CiError::new(format!("cannot read {}: {error}", to.display())))?;
        let meta = fs::symlink_metadata(entry.path()).map_err(|error| {
            CiError::new(format!(
                "cannot inspect {}: {error}",
                entry.path().display()
            ))
        })?;
        let name = entry.file_name();
        if !meta.is_file() || !names.iter().any(|known| name == *known) {
            return Err(CiError::new(format!(
                "{} holds {} besides the staged binaries; nothing may be added to the stage directory",
                to.display(),
                name.to_string_lossy()
            )));
        }
    }
    Ok(())
}

fn gate(args: &ImageGateArgs) -> Result<String, CiError> {
    let repo = Repo::parse(&args.repo)?;
    let github = http::HttpGitHub::new(api_url(), token()?, http::DEFAULT_TIMEOUT);
    let started = Instant::now();
    let clock = Clock {
        now: &|| started.elapsed(),
        sleep: &std::thread::sleep,
    };
    let outcome = if args.only == Some(GateOnly::Exemptions) {
        exemptions_only(&github, &repo, &args.pr_only_checks, &clock)?
    } else if args.only == Some(GateOnly::Tip) {
        tip_only(&github, &repo, &args.sha, &clock)?
    } else {
        let run_id = std::env::var("GITHUB_RUN_ID")
            .ok()
            .map(|id| id.parse::<u64>())
            .transpose()
            .map_err(|error| CiError::new(format!("invalid GITHUB_RUN_ID: {error}")))?;
        wait_for_required(
            &github,
            &repo,
            &Request {
                sha: &args.sha,
                require_tip: args.require_tip,
                wait: Wait {
                    total: Duration::from_secs(args.wait_seconds),
                    interval: Duration::from_secs(args.interval_seconds),
                },
                run_id,
                pr_only_checks: &args.pr_only_checks,
                verify_exemptions: args.verify_exemptions,
                ignore_jobs: &args.ignore_jobs,
                require_published: args.require_published.as_deref(),
            },
            &clock,
        )?
    };
    append_outputs(&format!("ready={}\n", outcome.ready))?;
    Ok(outcome.report)
}

fn changes(args: &ImageChangesArgs) -> Result<String, CiError> {
    let repo = Repo::parse(&args.repo)?;
    let github = http::HttpGitHub::new(api_url(), token()?, http::DEFAULT_TIMEOUT);
    let pairs = parse_matches(&args.matches)?;
    let number = match args.pull_request.as_deref() {
        None | Some("") => None,
        Some(text) => Some(
            text.parse::<u64>()
                .map_err(|_| CiError::new(format!("--pull-request {text} is not a number")))?,
        ),
    };
    let selection = select(&github, &repo, &args.event, number, &pairs)?;
    let mut outputs = String::new();
    for (name, selected) in &selection.images {
        let _ = writeln!(outputs, "{name}={selected}");
    }
    append_outputs(&outputs)?;
    Ok(selection.report)
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
    Ok(outcome.report)
}

fn authorize_release(args: &ImageAuthorizeArgs) -> Result<String, CiError> {
    let repo = Repo::parse(&args.repo)?;
    let github = http::HttpGitHub::new(api_url(), token()?, http::DEFAULT_TIMEOUT);
    authorize(
        &github,
        &repo,
        &Release {
            tag: &args.tag,
            actor: &args.actor,
            actors: &args.release_actors,
            run_id: std::env::var("GITHUB_RUN_ID")
                .ok()
                .map(|id| {
                    id.parse().map_err(|_| {
                        CiError::new(format!(
                            "GITHUB_RUN_ID `{id}` is not a number, so the time of the push is unknown and no ruleset can vouch; list the actor in release_actors or run this in a workflow run"
                        ))
                    })
                })
                .transpose()?,
        },
    )
}

fn advance_branch(args: &ImageAdvanceArgs) -> Result<String, CiError> {
    let repo = Repo::parse(&args.repo)?;
    let github = http::HttpGitHub::new(api_url(), token()?, http::DEFAULT_TIMEOUT);
    let advanced = advance(
        &github,
        &repo,
        &registry::Docker,
        &Advancement {
            images: &args.images,
            branch: &args.branch,
            commit: &args.commit,
            digest: &args.digest,
        },
    )?;
    append_outputs(&format!("moved={}\n", advanced.moved))?;
    Ok(advanced.report)
}

fn promote_release(args: &ImagePromoteArgs) -> Result<String, CiError> {
    let repo = Repo::parse(&args.repo)?;
    let github = http::HttpGitHub::new(api_url(), token()?, http::DEFAULT_TIMEOUT);
    let live = || live_tag_commit(&github, &repo, &args.tag);
    promote(
        &registry::Docker,
        &Promotion {
            images: &args.images,
            version: &args.version,
            commit: &args.commit,
            live_commit: &live,
        },
    )
}
