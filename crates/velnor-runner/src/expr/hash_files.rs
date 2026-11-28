// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use super::error::ExprError;
use super::glob::{PatternSet, WalkOptions};
use super::pattern::GlobError;
use super::pattern::trim_line;
use super::seam::Seam;
use super::value::Value;
use ring::digest::{Context, Digest, SHA256};
use std::fmt::{Display, Write as _};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const READ_BUFFER_BYTES: usize = 64 * 1024;
const LONGEST_TIMEOUT: Duration = Duration::from_hours(365 * 24);

#[derive(Clone)]
struct Job {
    workspace: PathBuf,
    home: Option<PathBuf>,
    arguments: Vec<String>,
    timeout: Duration,
    deadline: Instant,
    seam: Seam,
}

impl Job {
    fn escaped(&self) -> String {
        self.arguments.join(", ").replace('\'', "''")
    }

    fn failure(&self, detail: &dyn Display) -> ExprError {
        ExprError::evaluation(format!(
            "hashFiles('{}') failed. Fail to hash files under directory '{}'. {detail}",
            self.escaped(),
            self.workspace.display()
        ))
    }

    fn timed_out(&self) -> ExprError {
        ExprError::evaluation(format!(
            "hashFiles('{}') couldn't finish within {} seconds.",
            self.escaped(),
            self.timeout.as_secs()
        ))
    }

    fn run(&self) -> Result<String, ExprError> {
        let root = fs::canonicalize(&self.workspace).map_err(|error| self.failure(&error))?;
        let lines = self
            .arguments
            .iter()
            .flat_map(|argument| argument.split('\n'))
            .map(trim_line)
            .filter(|line| !line.is_empty() && !line.starts_with('#'));
        let patterns = PatternSet::new(&root, self.home.as_deref(), lines)
            .map_err(|error| self.failure(&error))?;

        let mut buffer = vec![0_u8; READ_BUFFER_BYTES];
        let mut aggregate = Context::new(&SHA256);
        let mut count = 0_usize;
        for found in patterns.walk(&root, self.deadline, &self.seam, WalkOptions::HASH_FILES) {
            let mut found = match found {
                Ok(found) => found,
                Err(GlobError::TimedOut) => return Err(self.timed_out()),
                Err(error) => return Err(self.failure(&error)),
            };
            match file_digest(&mut found.file, &mut buffer, self.deadline) {
                Ok(Some(digest)) => {
                    aggregate.update(digest.as_ref());
                    count += 1;
                }
                Ok(None) => return Err(self.timed_out()),
                Err(error) => {
                    return Err(self.failure(&format!("{}: {error}", found.path.display())));
                }
            }
        }
        if count == 0 {
            return Ok(String::new());
        }
        Ok(hex_digest(aggregate.finish().as_ref()))
    }
}

pub(crate) fn hash_files(
    workspace: &Path,
    home: Option<&Path>,
    args: &[Value],
    timeout: Duration,
    seam: &Seam,
) -> Result<String, ExprError> {
    let arguments: Vec<String> = args.iter().map(Value::convert_to_string).collect();
    if let Some(option) = arguments.first().filter(|first| first.starts_with("--")) {
        return Err(ExprError::evaluation(format!(
            "Invalid glob option {option}"
        )));
    }
    let job = Job {
        workspace: workspace.to_path_buf(),
        home: home.map(Path::to_path_buf),
        arguments,
        timeout,
        deadline: Instant::now() + timeout.min(LONGEST_TIMEOUT),
        seam: seam.clone(),
    };
    let (sender, receiver) = mpsc::channel();
    let worker = job.clone();
    thread::Builder::new()
        .name("hash-files".to_owned())
        .spawn(move || {
            let _ = sender.send(worker.run());
        })
        .map_err(|error| job.failure(&error))?;
    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(job.timed_out()),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(job.failure(&"the hashing thread ended without a result"))
        }
    }
}

fn file_digest(
    file: &mut File,
    buffer: &mut [u8],
    deadline: Instant,
) -> io::Result<Option<Digest>> {
    let mut context = Context::new(&SHA256);
    loop {
        if Instant::now() >= deadline {
            return Ok(None);
        }
        let read = file.read(buffer)?;
        if read == 0 {
            return Ok(Some(context.finish()));
        }
        context.update(&buffer[..read]);
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}
