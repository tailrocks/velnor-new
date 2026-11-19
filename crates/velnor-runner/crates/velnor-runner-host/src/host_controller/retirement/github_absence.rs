// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The removal-proof record of a removed connection.
//!
//! `connection-drain/<id>~github-absent` lists the `scale_set_id:runner_id` pairs that the
//! removal flow deregistered and then confirmed absent on GitHub. It is bound to the removal by
//! the same token and binding snapshot as the drain proof, so it is valid only while that drain
//! proof is current and the connection is still absent from the host config. The legacy
//! reconcile gate reads it as proof that those runners are gone; a removed connection without
//! it stays held.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::config::{ConnectionId, HostConfig, RepositoryConnection};
use crate::error::HostError;

use super::{
    MAX_BINDING_BYTES, MAX_SIDECARS, parse_binding, proof_is_current, read_regular_file,
    read_token, trusted_private_directory,
};

const SUFFIX: &str = "~github-absent";
const MAX_PAIRS: usize = 4096;
const MAX_RECORD_BYTES: u64 = 32 + 1 + (MAX_PAIRS as u64) * 42;

/// `(scale_set_id, runner_id)` pairs proven absent from GitHub by a removal flow.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct AbsenceProofs(BTreeSet<(i64, i64)>);

impl AbsenceProofs {
    /// Whether a removed connection's record proves `runner` absent from `scale_set_id`.
    /// `runner` must be the canonical decimal id the journal row stores.
    pub(crate) fn proves(&self, scale_set_id: i64, runner: &str) -> bool {
        runner
            .parse::<i64>()
            .is_ok_and(|id| id.to_string() == runner && self.0.contains(&(scale_set_id, id)))
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, scale_set_id: i64, runner_id: i64) -> bool {
        self.0.contains(&(scale_set_id, runner_id))
    }
}

/// Reads every valid removal-proof record. A record counts only when its connection is absent
/// from `config` and its drain proof is current; anything unreadable is skipped, so a damaged
/// record leaves its rows held rather than released.
pub(crate) fn read_absence_proofs(state: &Path, config: &HostConfig) -> AbsenceProofs {
    let directory = state.join("connection-drain");
    let Ok(entries) = fs::read_dir(&directory) else {
        return AbsenceProofs::default();
    };
    let mut pairs = BTreeSet::new();
    for entry in entries.take(MAX_SIDECARS).flatten() {
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|name| name.strip_suffix(SUFFIX))
            .and_then(|id| ConnectionId::parse(id).ok())
        else {
            continue;
        };
        if config.connection(&id).is_some() {
            continue;
        }
        match proven_for_removed(state, &directory, &id) {
            Ok(proven) => pairs.extend(proven),
            Err(error) => eprintln!(
                "host controller removal proof ignored connection={} reason={error}",
                id.as_str()
            ),
        }
    }
    AbsenceProofs(pairs)
}

fn proven_for_removed(
    state: &Path,
    directory: &Path,
    id: &ConnectionId,
) -> Result<BTreeSet<(i64, i64)>, HostError> {
    let snapshot = read_regular_file(
        &directory.join(format!("{}~binding", id.as_str())),
        MAX_BINDING_BYTES,
    )?
    .ok_or(HostError::Path)?;
    let binding = parse_binding(&snapshot).ok_or(HostError::Path)?;
    if &binding.connection_id != id {
        return Err(HostError::Path);
    }
    proven_for(state, &binding)
}

fn proven_for(
    state: &Path,
    binding: &RepositoryConnection,
) -> Result<BTreeSet<(i64, i64)>, HostError> {
    if !proof_is_current(state, binding)? {
        return Err(HostError::Path);
    }
    let directory = state.join("connection-drain");
    let id = binding.connection_id.as_str();
    let Some(record) =
        read_regular_file(&directory.join(format!("{id}{SUFFIX}")), MAX_RECORD_BYTES)?
    else {
        return Ok(BTreeSet::new());
    };
    let token = read_token(&directory.join(id))?.ok_or(HostError::Path)?;
    parse_record(&record, &token).ok_or(HostError::Path)
}

fn parse_record(record: &[u8], token: &str) -> Option<BTreeSet<(i64, i64)>> {
    let text = std::str::from_utf8(record).ok()?;
    let mut lines = text.lines();
    if lines.next()? != token {
        return None;
    }
    let mut pairs = BTreeSet::new();
    for line in lines {
        let (set, runner) = line.split_once(':')?;
        let pair = (set.parse::<i64>().ok()?, runner.parse::<i64>().ok()?);
        if pair.0 <= 0 || pair.1 <= 0 || format!("{}:{}", pair.0, pair.1) != line {
            return None;
        }
        pairs.insert(pair);
    }
    (pairs.len() <= MAX_PAIRS).then_some(pairs)
}

/// The pairs this connection's record already holds. A missing, damaged or stale record is
/// empty, so the removal flow proves them again.
pub(crate) fn recorded_pairs(state: &Path, binding: &RepositoryConnection) -> BTreeSet<(i64, i64)> {
    proven_for(state, binding).unwrap_or_default()
}

/// Adds `proven` to the connection's removal-proof record. The caller has already deregistered
/// each pair and confirmed it absent on GitHub; nothing else may call this. A binding whose
/// drain proof is not current gets no record.
pub(crate) fn record_absence(
    state: &Path,
    binding: &RepositoryConnection,
    proven: &BTreeSet<(i64, i64)>,
) -> Result<(), HostError> {
    if proven.is_empty() || !proof_is_current(state, binding)? {
        return Ok(());
    }
    let directory = state.join("connection-drain");
    let metadata = fs::symlink_metadata(&directory).map_err(|_| HostError::Path)?;
    if !trusted_private_directory(&metadata) {
        return Err(HostError::Path);
    }
    let id = binding.connection_id.as_str();
    let token = read_token(&directory.join(id))?.ok_or(HostError::Path)?;
    let mut pairs = recorded_pairs(state, binding);
    pairs.extend(proven.iter().copied());
    if pairs.len() > MAX_PAIRS {
        return Err(HostError::Path);
    }
    let mut record = format!("{token}\n");
    for (set, runner) in &pairs {
        record.extend([
            set.to_string(),
            ":".to_owned(),
            runner.to_string(),
            "\n".to_owned(),
        ]);
    }
    let target = directory.join(format!("{id}{SUFFIX}"));
    let scratch = directory.join(format!("{id}{SUFFIX}.tmp"));
    match fs::remove_file(&scratch) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(HostError::Path),
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&scratch)
        .map_err(|_| HostError::Path)?;
    file.write_all(record.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|_| HostError::Path)?;
    fs::rename(&scratch, &target).map_err(|_| HostError::Path)?;
    fs::File::open(&directory)
        .and_then(|dir| dir.sync_all())
        .map_err(|_| HostError::Path)
}

/// Writes the marker, binding snapshot and drain proof the CLI leaves when it removes a
/// connection, so tests can start from a removed connection with a current drain proof.
#[cfg(test)]
pub(crate) fn removed_fixture(state: &Path, binding: &RepositoryConnection) {
    use std::os::unix::fs::PermissionsExt;
    const TOKEN: &str = "0123456789abcdef0123456789abcdef";
    let directory = state.join("connection-drain");
    fs::create_dir_all(&directory).expect("drain directory");
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).expect("drain mode");
    let snapshot = binding.canonical_identity_bytes().expect("identity");
    let mut proof = format!("{TOKEN}\n").into_bytes();
    proof.extend_from_slice(&snapshot);
    let id = binding.connection_id.as_str();
    for (name, bytes) in [
        (id.to_owned(), TOKEN.as_bytes()),
        (format!("{id}~binding"), snapshot.as_slice()),
        (format!("{id}~drained"), proof.as_slice()),
    ] {
        let path = directory.join(name);
        fs::write(&path, bytes).expect("drain file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("drain file mode");
    }
}

#[cfg(test)]
const FIXTURE_CONNECTION: &str = r#"[[connections]]
connection_id = "conn-test-1"
repository_id = 42
repository = "owner/repo"
credential_ref = "keychain:velnor.test/conn-test-1"
[connections.scale_set]
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
[connections.source_policy]
mode = "branch_restricted"
branches = ["main"]
tag_prefixes = []
events = ["push"]
pull_request_rules = []
"#;

#[cfg(test)]
const FIXTURE_HOST: &str = r#"schema = 2
[host]
max_jobs = 1
[host.resources]
runner_cpu_millicores = 2000
runner_memory_bytes = 4294967296
dind_cpu_millicores = 1000
dind_memory_bytes = 2147483648
operational_memory_reserve_bytes = 1073741824
[docker]
context = "orbstack"
platform = "linux/amd64"
endpoint = "unix:///docker.sock"
"#;

/// The host config holding the single fixture connection `id`.
#[cfg(test)]
pub(crate) fn fixture_config(id: &str) -> Result<HostConfig, Box<dyn std::error::Error>> {
    let connection = FIXTURE_CONNECTION.replace("conn-test-1", id);
    Ok(HostConfig::parse(&FIXTURE_HOST.replacen(
        "[host]",
        &format!("{connection}[host]"),
        1,
    ))?)
}
