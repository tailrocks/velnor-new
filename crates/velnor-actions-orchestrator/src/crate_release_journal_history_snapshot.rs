// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Seek, SeekFrom, Write};

use super::{
    JournalIdentity, JournalIo, JournalOperationRefs, JournalStage, OrchestratorError,
    PriorHistoryEvidence, inspect_journal, inspect_recoverable_history, invalid, journal_name,
};

pub(crate) struct PriorHistorySnapshot {
    spool: tempfile::NamedTempFile,
    deadline: velnor_actions_mise::CheckDeadline,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Node {
    identity: JournalIdentity,
    predecessor: Option<JournalIdentity>,
    start_ref: String,
    start: String,
    mutations: String,
    references: String,
}

fn fingerprint<T: serde::Serialize>(value: &T) -> Result<String, OrchestratorError> {
    let bytes = velnor_actions_contract::canonical_json_bytes(value).map_err(|_| invalid())?;
    Ok(crate::planner::generator::sha256_hex(&bytes))
}

fn references<I: JournalIo + JournalOperationRefs>(
    io: &I,
    identity: &JournalIdentity,
) -> Result<Vec<(u32, String)>, OrchestratorError> {
    super::history::operation_sequences(io, identity)
}

impl Node {
    fn capture<I: JournalIo + JournalOperationRefs>(
        io: &I,
        evidence: &PriorHistoryEvidence,
    ) -> Result<Self, OrchestratorError> {
        let identity = &evidence.start.receipt.identity;
        let reference = journal_name(identity, JournalStage::Started);
        let start_ref = io
            .read_ref_with_access(&identity.repository, &reference)?
            .ok_or_else(invalid)?;
        let before = references(io, identity)?;
        if inspect_journal(io, identity, JournalStage::Started)?.as_ref() != Some(&evidence.start)
            || inspect_recoverable_history(io, identity, &evidence.start.receipt)?
                != (evidence.mutations.clone(), None)
            || references(io, identity)? != before
            || io
                .read_ref_with_access(&identity.repository, &reference)?
                .as_ref()
                != Some(&start_ref)
        {
            return Err(invalid());
        }
        Ok(Self {
            identity: identity.clone(),
            predecessor: evidence.start.receipt.predecessor.clone(),
            start_ref,
            start: fingerprint(&evidence.start.receipt)?,
            mutations: fingerprint(&evidence.mutations)?,
            references: fingerprint(&before)?,
        })
    }
}

impl PriorHistorySnapshot {
    pub(crate) fn revalidate<I: JournalIo + JournalOperationRefs>(
        &self,
        io: &I,
        current: &super::JournalReceipt,
    ) -> Result<(), OrchestratorError> {
        self.deadline
            .scope(|| {
                let mut next = current.predecessor.clone();
                self.visit(|node| {
                    if next.as_ref() != Some(&node.identity) {
                        return Err(invalid());
                    }
                    let start = inspect_journal(io, &node.identity, JournalStage::Started)?
                        .ok_or_else(invalid)?;
                    let (mutations, completed) =
                        inspect_recoverable_history(io, &node.identity, &start.receipt)?;
                    if completed.is_some()
                        || Node::capture(io, &PriorHistoryEvidence { start, mutations })? != node
                    {
                        return Err(invalid());
                    }
                    next = node.predecessor;
                    Ok(())
                })?;
                if next.is_some() {
                    return Err(invalid());
                }
                self.check()
            })
            .map_err(|_| invalid())?
    }

    pub(crate) fn matches(&self, other: &Self) -> Result<bool, OrchestratorError> {
        self.check()?;
        other.check()?;
        let mut left = self.spool.reopen().map_err(|_| invalid())?;
        let mut right = other.spool.reopen().map_err(|_| invalid())?;
        left.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
        right.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
        let mut a = [0_u8; 4096];
        let mut b = [0_u8; 4096];
        loop {
            self.check()?;
            other.check()?;
            let count = left.read(&mut a).map_err(|_| invalid())?;
            if count == 0 {
                let matches = right
                    .read(&mut b[..1])
                    .map(|count| count == 0)
                    .map_err(|_| invalid())?;
                self.check()?;
                other.check()?;
                return Ok(matches);
            }
            if right.read_exact(&mut b[..count]).is_err() || a[..count] != b[..count] {
                return Ok(false);
            }
        }
    }

    pub(super) fn new(
        deadline: velnor_actions_mise::CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        Ok(Self {
            spool: tempfile::NamedTempFile::new().map_err(|_| invalid())?,
            deadline,
        })
    }

    fn check(&self) -> Result<(), OrchestratorError> {
        self.deadline.remaining().map(|_| ()).map_err(|_| invalid())
    }

    pub(super) fn contains(&self, identity: &JournalIdentity) -> Result<bool, OrchestratorError> {
        let mut found = false;
        self.visit(|node| {
            found |= (node.identity.run_id, node.identity.run_attempt)
                == (identity.run_id, identity.run_attempt);
            Ok(())
        })?;
        Ok(found)
    }

    pub(super) fn pin<I: JournalIo + JournalOperationRefs>(
        &mut self,
        io: &I,
        evidence: &PriorHistoryEvidence,
    ) -> Result<(), OrchestratorError> {
        self.check()?;
        let bytes = velnor_actions_contract::canonical_json_bytes(&Node::capture(io, evidence)?)
            .map_err(|_| invalid())?;
        let length = u32::try_from(bytes.len()).map_err(|_| invalid())?;
        if length == 0 || length > 4096 {
            return Err(invalid());
        }
        self.check()?;
        self.spool
            .as_file_mut()
            .write_all(&length.to_be_bytes())
            .and_then(|()| self.spool.as_file_mut().write_all(&bytes))
            .map_err(|_| invalid())
    }

    fn visit(
        &self,
        mut visit: impl FnMut(Node) -> Result<(), OrchestratorError>,
    ) -> Result<(), OrchestratorError> {
        let mut file = self.spool.reopen().map_err(|_| invalid())?;
        file.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
        loop {
            self.check()?;
            let mut length = [0_u8; 4];
            match file.read(&mut length[..1]).map_err(|_| invalid())? {
                0 => return Ok(()),
                1 => (),
                _ => return Err(invalid()),
            }
            file.read_exact(&mut length[1..]).map_err(|_| invalid())?;
            let length = u32::from_be_bytes(length) as usize;
            if length == 0 || length > 4096 {
                return Err(invalid());
            }
            let mut bytes = vec![0_u8; length];
            file.read_exact(&mut bytes).map_err(|_| invalid())?;
            visit(serde_json::from_slice(&bytes).map_err(|_| invalid())?)?;
            self.check()?;
        }
    }
}
