use super::ledger::{AdmissionOwner, EntryRecord, TerminalOutcome, lock, publish, read};
use eyre::{Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Native capability over exactly the events accepted before atomic close.
/// It says nothing about later wrappers, descendants, or whole-task zero.
#[derive(Debug, Serialize)]
pub(crate) struct AdmissionClosure {
    schema_version: u8,
    scope: &'static str,
    lifetime: &'static str,
    session_id: String,
    root_session_id: String,
    accepted_count: u64,
    acknowledged_count: u64,
    failed_count: u64,
    outstanding_count: u64,
    inventory_sha256: String,
    entries: Vec<AcceptedEntry>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AcceptedEntry {
    pub(crate) event_id: String,
    pub(crate) adapter: mbx_cache_core::AdapterKind,
    pub(crate) kind: super::ledger::AdmissionKind,
    pub(crate) terminal: Option<TerminalOutcome>,
    pub(crate) dynamic_evidence: Option<serde_json::Value>,
}

impl AdmissionClosure {
    pub(crate) fn identity_matches(&self, session: &str, root: &str) -> bool {
        self.session_id == session && self.root_session_id == root
    }

    pub(crate) fn closed_successfully(&self) -> bool {
        self.failed_count == 0
            && self.outstanding_count == 0
            && self.accepted_count == self.acknowledged_count
    }

    pub(crate) fn scope(&self) -> &'static str {
        self.scope
    }
    pub(crate) fn lifetime(&self) -> &'static str {
        self.lifetime
    }
    pub(crate) fn accepted_entries(&self) -> impl Iterator<Item = &AcceptedEntry> {
        self.entries.iter()
    }
    pub(crate) fn ledger_inventory_sha256(&self) -> &str {
        &self.inventory_sha256
    }
}

impl AdmissionOwner {
    pub(crate) fn close(self) -> Result<AdmissionClosure> {
        let _lock = lock(&self.directory)?;
        publish(
            &self.directory,
            "closed.json",
            &serde_json::json!({
                "schema_version": 1,
                "session_id": self.identity.session_id,
                "root_session_id": self.identity.root_session_id,
                "lifetime": "accepted_before_close",
            }),
        )?;
        let mut paths = std::fs::read_dir(&self.directory)?
            .take(100_001)
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        if paths.len() > 100_000 {
            bail!("native admission inventory exceeds bound");
        }
        let mut bytes = 0u64;
        for path in &paths {
            bytes = bytes
                .checked_add(std::fs::symlink_metadata(path)?.len())
                .ok_or_else(|| eyre::eyre!("native admission inventory byte overflow"))?;
            if bytes > 64 * 1024 * 1024 {
                bail!("native admission inventory exceeds aggregate byte bound");
            }
        }
        let mut closure = AdmissionClosure {
            schema_version: 1,
            scope: "mbx_session_admissions",
            lifetime: "accepted_before_close",
            session_id: self.identity.session_id,
            root_session_id: self.identity.root_session_id,
            accepted_count: 0,
            acknowledged_count: 0,
            failed_count: 0,
            outstanding_count: 0,
            inventory_sha256: String::new(),
            entries: Vec::new(),
        };
        scan(&self.directory, paths, &mut closure)?;
        publish(&self.directory, "seal.json", &closure)?;
        Ok(closure)
    }
}

fn scan(
    directory: &std::path::Path,
    paths: Vec<std::path::PathBuf>,
    closure: &mut AdmissionClosure,
) -> Result<()> {
    let mut inventory = Sha256::new();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| eyre::eyre!("invalid admission inventory path"))?;
        if let Some(event) = name
            .strip_suffix(".terminal.json")
            .or_else(|| name.strip_suffix(".evidence.json"))
        {
            if !directory.join(format!("{event}.accepted.json")).is_file() {
                bail!("orphan native admission terminal or evidence");
            }
        }
        if !name.ends_with(".accepted.json") {
            if !matches!(name, "identity.json" | "closed.json" | "ledger.lock")
                && !name.ends_with(".terminal.json")
                && !name.ends_with(".evidence.json")
            {
                bail!("unknown native admission inventory member");
            }
            continue;
        }
        let bytes = read(&path)?;
        let entry: EntryRecord = serde_json::from_slice(&bytes)?;
        super::ledger::validate_id(&entry.session_id, &entry.root_session_id, &entry.event_id)?;
        if matches!(
            entry.kind,
            super::ledger::AdmissionKind::BuildScriptProvision
        ) && entry.adapter != mbx_cache_core::AdapterKind::BuildScript
            || matches!(entry.kind, super::ledger::AdmissionKind::CcSelection)
                && entry.adapter != mbx_cache_core::AdapterKind::Cc
        {
            bail!("native admission adapter/kind mismatch");
        }
        if entry.session_id != closure.session_id
            || entry.root_session_id != closure.root_session_id
            || name != format!("{}.accepted.json", entry.event_id)
        {
            bail!("admission inventory identity mismatch");
        }
        inventory.update((bytes.len() as u64).to_le_bytes());
        inventory.update(&bytes);
        closure.accepted_count += 1;
        let terminal = terminal(directory, &entry.event_id, closure, &mut inventory)?;
        closure.entries.push(AcceptedEntry {
            dynamic_evidence: dynamic_evidence(
                directory,
                &entry.event_id,
                terminal.as_ref(),
                &mut inventory,
            )?,
            event_id: entry.event_id,
            adapter: entry.adapter,
            kind: entry.kind,
            terminal,
        });
    }
    closure.inventory_sha256 = hex::encode(inventory.finalize());
    Ok(())
}

fn dynamic_evidence(
    directory: &std::path::Path,
    event: &str,
    terminal: Option<&TerminalOutcome>,
    inventory: &mut Sha256,
) -> Result<Option<serde_json::Value>> {
    let path = directory.join(format!("{event}.evidence.json"));
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => bail!("dynamic admission payload is not a regular file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let bytes = read(&path)?;
    let digest = hex::encode(Sha256::digest(&bytes));
    if let Some(TerminalOutcome::Acknowledged { event_sha256 }) = terminal {
        if *event_sha256 != digest {
            bail!("dynamic admission payload digest mismatch");
        }
    }
    inventory.update((bytes.len() as u64).to_le_bytes());
    inventory.update(&bytes);
    Ok(Some(serde_json::from_slice(&bytes)?))
}

fn terminal(
    directory: &std::path::Path,
    event: &str,
    closure: &mut AdmissionClosure,
    inventory: &mut Sha256,
) -> Result<Option<TerminalOutcome>> {
    let path = directory.join(format!("{event}.terminal.json"));
    if !path.exists() {
        closure.outstanding_count += 1;
        return Ok(None);
    }
    let bytes = read(&path)?;
    inventory.update((bytes.len() as u64).to_le_bytes());
    inventory.update(&bytes);
    let outcome: TerminalOutcome = serde_json::from_slice(&bytes)?;
    match &outcome {
        TerminalOutcome::Acknowledged { event_sha256 }
            if super::ledger::valid_digest(event_sha256) =>
        {
            closure.acknowledged_count += 1;
        }
        TerminalOutcome::Failed { reason } if !reason.is_empty() && reason.len() <= 1024 => {
            closure.failed_count += 1;
        }
        _ => bail!("invalid admitted terminal"),
    }
    Ok(Some(outcome))
}
