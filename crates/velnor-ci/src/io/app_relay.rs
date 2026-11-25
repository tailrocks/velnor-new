// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use crypto_box::aead::OsRng;
use zeroize::Zeroizing;

use super::app_token::{mint_token, publish_token, require_output, revoke_quietly};
use super::artifact_api::ArtifactApi;
use super::artifact_unzip::{Budget, extract};
use super::{OUTPUT_VARIABLE, append_to};
use crate::app_relay::{Binding, PublicKeyDocument, generate, open, seal};
use crate::cli::{AwaitArgs, JobArgs, RelayArgs, WaitArgs};
use crate::error::CiError;
use crate::github::Repo;

const PUBLIC_MEMBER: &str = "public-key.json";
const SEALED_MEMBER: &str = "token.sealed";
const EXTRACT_LIMIT: u64 = 1 << 20;

struct Handshake {
    binding: Binding,
    repo: Repo,
    root: PathBuf,
}

impl Handshake {
    fn of(job: &JobArgs) -> Result<Self, CiError> {
        let variable = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| CiError::new(format!("{name} is not set")))
        };
        let number = |name: &str| {
            variable(name)?
                .parse::<u64>()
                .map_err(|_| CiError::new(format!("{name} is not a number")))
        };
        let repository = variable("GITHUB_REPOSITORY")?;
        let temp = PathBuf::from(variable("RUNNER_TEMP")?);
        if !temp.is_absolute() {
            return Err(CiError::new("RUNNER_TEMP is not an absolute path"));
        }
        Ok(Self {
            binding: Binding::new(
                &repository,
                number("GITHUB_RUN_ID")?,
                number("GITHUB_RUN_ATTEMPT")?,
                &job.job,
            )?,
            repo: Repo::parse(&repository)?,
            root: temp.join("velnor-app-token"),
        })
    }

    fn directory(&self) -> PathBuf {
        self.root.join(&self.binding.job)
    }

    fn key_file(&self) -> PathBuf {
        self.root.join(format!("{}.key", self.binding.job))
    }

    fn fresh_directory(&self) -> Result<PathBuf, CiError> {
        let directory = self.directory();
        remove_tree(&directory)?;
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)
            .map_err(|e| CiError::new(format!("cannot create {}: {e}", directory.display())))?;
        Ok(directory)
    }
}

fn announce(artifact: &str, path: &Path) -> Result<(), CiError> {
    append_to(
        OUTPUT_VARIABLE,
        &format!("artifact={artifact}\npath={}\n", path.display()),
    )
}

fn remove_tree(path: &Path) -> Result<(), CiError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CiError::new(format!(
            "cannot remove {}: {error}",
            path.display()
        ))),
    }
}

fn write_private(path: &Path, content: &str) -> Result<(), CiError> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CiError::new(format!(
                "cannot remove {}: {error}",
                path.display()
            )));
        }
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(content.as_bytes()))
        .map_err(|e| CiError::new(format!("cannot write {}: {e}", path.display())))
}

pub(super) fn request(job: &JobArgs) -> Result<String, CiError> {
    require_output()?;
    let handshake = Handshake::of(job)?;
    let (secret, document) = generate(&mut OsRng, &handshake.binding);
    let directory = handshake.fresh_directory()?;
    write_private(
        &handshake.key_file(),
        &Zeroizing::new(STANDARD.encode(secret.as_slice())),
    )?;
    let file = directory.join(PUBLIC_MEMBER);
    write_private(&file, &document.to_json()?)?;
    announce(&handshake.binding.public_artifact(), &file)?;
    Ok(format!(
        "Wrote the public key for job {} of run {} attempt {}\n",
        handshake.binding.job, handshake.binding.run_id, handshake.binding.attempt
    ))
}

enum Lookup {
    Absent,
    One(u64),
    Squatted(usize),
}

fn lookup(api: &ArtifactApi, handshake: &Handshake, name: &str) -> Result<Lookup, CiError> {
    let live: Vec<u64> = api
        .list(&handshake.repo, handshake.binding.run_id)?
        .into_iter()
        .filter(|artifact| artifact.name == name && !artifact.expired)
        .map(|artifact| artifact.id)
        .collect();
    Ok(match live.as_slice() {
        [] => Lookup::Absent,
        [id] => Lookup::One(*id),
        many => Lookup::Squatted(many.len()),
    })
}

fn read_member(
    api: &ArtifactApi,
    handshake: &Handshake,
    (id, name, member): (u64, &str, &str),
) -> Result<Vec<u8>, CiError> {
    let stage = handshake.root.join(format!("{}-in", handshake.binding.job));
    remove_tree(&stage)?;
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&stage)
        .map_err(|e| CiError::new(format!("cannot create {}: {e}", stage.display())))?;
    let archive = api.download(&handshake.repo, id, &stage.join("download.zip"))?;
    let extracted = extract(
        BufReader::new(archive),
        name,
        &stage,
        &Budget::new(EXTRACT_LIMIT),
    )
    .and_then(|()| {
        let file = stage.join(name).join(member);
        fs::read(&file).map_err(|e| CiError::new(format!("cannot read {}: {e}", file.display())))
    });
    remove_tree(&stage)?;
    extracted
}

fn wait_for(
    handshake: &Handshake,
    wait: &WaitArgs,
    (name, member): (&str, &str),
) -> Result<Vec<u8>, CiError> {
    let api = ArtifactApi::new(super::api_url(), super::token()?);
    let deadline = Instant::now() + Duration::from_secs(wait.timeout_seconds);
    let mut last = None;
    loop {
        match lookup(&api, handshake, name) {
            Ok(Lookup::One(id)) => match read_member(&api, handshake, (id, name, member)) {
                Ok(bytes) => return Ok(bytes),
                Err(error) => last = Some(error),
            },
            Ok(Lookup::Squatted(count)) => {
                return Err(CiError::new(format!(
                    "{count} live artifacts are named {name}; another job in this run uploaded one, so the handshake is refused"
                )));
            }
            Ok(Lookup::Absent) => {}
            Err(error) => last = Some(error),
        }
        let now = Instant::now();
        if now >= deadline {
            let seen = last.map_or_else(String::new, |error| format!(" (last error: {error})"));
            return Err(CiError::new(format!(
                "no artifact {name} appeared within {} seconds{seen}",
                wait.timeout_seconds
            )));
        }
        std::thread::sleep(Duration::from_secs(wait.poll_seconds).min(deadline - now));
    }
}

pub(super) fn relay(args: &RelayArgs) -> Result<String, CiError> {
    let handshake = Handshake::of(&args.job)?;
    let bytes = wait_for(
        &handshake,
        &args.wait,
        (&handshake.binding.public_artifact(), PUBLIC_MEMBER),
    )?;
    let document = PublicKeyDocument::parse(&bytes, &handshake.binding)?;
    let token = mint_token(&args.mint)?;
    let sealed = seal(&mut OsRng, &document, token.as_str())
        .map_err(|error| revoke_quietly(error, token.as_str()))?;
    drop(token);
    let directory = handshake.fresh_directory()?;
    let file = directory.join(SEALED_MEMBER);
    write_private(&file, &sealed)?;
    announce(&handshake.binding.token_artifact(), &file)?;
    Ok(format!(
        "Sealed a token with {} permissions for job {}\n",
        args.mint.permissions.len(),
        handshake.binding.job
    ))
}

pub(super) fn wait(args: &AwaitArgs) -> Result<String, CiError> {
    require_output()?;
    let handshake = Handshake::of(&args.job)?;
    let key_file = handshake.key_file();
    let secret = fs::read_to_string(&key_file)
        .map(Zeroizing::new)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", key_file.display())));
    let removed = remove_key(&key_file);
    let secret = secret?;
    removed?;
    let secret = STANDARD
        .decode(secret.trim())
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .map(Zeroizing::new)
        .ok_or_else(|| CiError::new("the private key file is not 32 bytes of base64"))?;
    let bytes = wait_for(
        &handshake,
        &args.wait,
        (&handshake.binding.token_artifact(), SEALED_MEMBER),
    )?;
    let sealed = String::from_utf8(bytes)
        .map_err(|_| CiError::new("the sealed token artifact is not text"))?;
    let token = open(&secret, &sealed, &handshake.binding)?;
    publish_token(&token)?;
    remove_tree(&handshake.directory())?;
    Ok(format!(
        "Received the token of job {} from the relay\n",
        handshake.binding.job
    ))
}

fn remove_key(path: &Path) -> Result<(), CiError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CiError::new(format!(
            "cannot delete {}: {error}",
            path.display()
        ))),
    }
}
