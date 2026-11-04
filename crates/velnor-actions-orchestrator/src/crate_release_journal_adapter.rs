// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha1_checked::{Digest, Sha1};
use std::{cell::RefCell, collections::BTreeMap};

use crate::{
    OrchestratorError,
    crate_release_journal::{JournalIo, JournalTag},
    internal::internal,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTag {
    pub sha: String,
    pub tag: JournalTag,
    pub tagger_name: String,
    pub tagger_email: String,
    pub tagger_date: String,
    pub unsigned: bool,
}

pub trait JournalTransport {
    fn check_repository_access(&self, repository: &str) -> Result<(), OrchestratorError>;
    fn check_tag_rules(&self, repository: &str) -> Result<(), OrchestratorError>;
    fn get_ref(
        &self,
        repository: &str,
        reference: &str,
    ) -> Result<Option<String>, OrchestratorError>;
    fn read_ref_with_access(
        &self,
        repository: &str,
        reference: &str,
    ) -> Result<Option<String>, OrchestratorError> {
        self.check_repository_access(repository)?;
        let observed = self.get_ref(repository, reference)?;
        if observed.is_none() {
            self.check_repository_access(repository)?;
        }
        Ok(observed)
    }
    fn create_blob(&self, repository: &str, bytes: &[u8]) -> Result<String, OrchestratorError>;
    fn create_tag(
        &self,
        repository: &str,
        name: &str,
        blob_sha: &str,
    ) -> Result<String, OrchestratorError>;
    fn create_ref(
        &self,
        repository: &str,
        reference: &str,
        tag_sha: &str,
    ) -> Result<(), OrchestratorError>;
    fn get_tag(&self, repository: &str, sha: &str) -> Result<RemoteTag, OrchestratorError>;
    fn get_blob(&self, repository: &str, sha: &str) -> Result<Vec<u8>, OrchestratorError>;
}

pub trait JournalRefTransport {
    fn list_operations(
        &self,
        identity: &crate::crate_release_journal::JournalIdentity,
    ) -> Result<Vec<(String, String)>, OrchestratorError>;
}

type CacheKey = (String, String);
const MAX_CACHED_OBJECTS: usize = 128;
const MAX_CACHED_BLOB_BYTES: usize = 64 * 1024;

pub struct JournalAdapter<T> {
    transport: T,
    tags: RefCell<BTreeMap<CacheKey, (JournalTag, Vec<u8>)>>,
    blobs: RefCell<BTreeMap<CacheKey, Vec<u8>>>,
}

impl<T> JournalAdapter<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            tags: RefCell::new(BTreeMap::new()),
            blobs: RefCell::new(BTreeMap::new()),
        }
    }
}

impl<T: JournalTransport + JournalRefTransport> crate::crate_release_journal::JournalOperationRefs
    for JournalAdapter<T>
{
    fn list_operation_refs(
        &self,
        identity: &crate::crate_release_journal::JournalIdentity,
    ) -> Result<Vec<(String, String)>, OrchestratorError> {
        self.transport
            .check_repository_access(&identity.repository)?;
        let names = self.transport.list_operations(identity)?;
        let prefix = format!(
            "refs/tags/velnor-receipt/{}/{}/operation-",
            identity.run_id, identity.run_attempt
        );
        let mut unique = std::collections::BTreeMap::new();
        for (name, sha) in names {
            if !name.starts_with(&prefix)
                || !valid_reference(&name)
                || !valid_sha(&sha)
                || unique.insert(name, sha).is_some()
            {
                return Err(invalid());
            }
        }
        self.transport
            .check_repository_access(&identity.repository)?;
        Ok(unique.into_iter().collect())
    }
}

impl<T> std::fmt::Debug for JournalAdapter<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JournalAdapter")
            .finish_non_exhaustive()
    }
}

impl<T: JournalTransport> JournalAdapter<T> {
    fn verified_tag(
        &self,
        repository: &str,
        sha: &str,
    ) -> Result<(JournalTag, Vec<u8>), OrchestratorError> {
        if !valid_sha(sha) {
            return Err(invalid());
        }
        let key = (repository.to_owned(), sha.to_owned());
        if let Some(value) = self.tags.borrow().get(&key) {
            return Ok(value.clone());
        }
        let remote = self.transport.get_tag(repository, sha)?;
        if remote.sha != sha
            || !remote.unsigned
            || remote.tagger_name != "Velnor Release"
            || remote.tagger_email != "release@velnor.invalid"
            || remote.tagger_date != "1970-01-01T00:00:00Z"
            || !valid_tag(&remote.tag)
        {
            return Err(invalid());
        }
        let raw = canonical_tag(&remote.tag);
        let mut hash = Sha1::new();
        hash.update(format!("tag {}\0", raw.len()));
        hash.update(&raw);
        let digest = hash.try_finalize();
        if digest.has_collision() {
            return Err(invalid());
        }
        let computed = digest
            .hash()
            .iter()
            .fold(String::new(), |mut result, byte| {
                result.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
                result.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
                result
            });
        if computed != sha {
            return Err(invalid());
        }
        let value = (remote.tag, raw);
        let mut cache = self.tags.borrow_mut();
        if cache.len() < MAX_CACHED_OBJECTS {
            cache.insert(key, value.clone());
        }
        Ok(value)
    }
}

impl<T: JournalTransport> JournalIo for JournalAdapter<T> {
    fn check_repository_access(&self, repository: &str) -> Result<(), OrchestratorError> {
        self.transport.check_repository_access(repository)
    }
    fn check_tag_rules(&self, repository: &str) -> Result<(), OrchestratorError> {
        self.transport.check_tag_rules(repository)
    }
    fn get_ref(
        &self,
        repository: &str,
        reference: &str,
    ) -> Result<Option<String>, OrchestratorError> {
        if !valid_reference(reference) {
            return Err(invalid());
        }
        let result = self.transport.get_ref(repository, reference)?;
        if result.as_deref().is_some_and(|sha| !valid_sha(sha)) {
            return Err(invalid());
        }
        Ok(result)
    }
    fn read_ref_with_access(
        &self,
        repository: &str,
        reference: &str,
    ) -> Result<Option<String>, OrchestratorError> {
        if !valid_reference(reference) {
            return Err(invalid());
        }
        let result = self.transport.read_ref_with_access(repository, reference)?;
        if result.as_deref().is_some_and(|sha| !valid_sha(sha)) {
            return Err(invalid());
        }
        Ok(result)
    }
    fn create_blob(&self, repository: &str, bytes: &[u8]) -> Result<String, OrchestratorError> {
        self.transport.create_blob(repository, bytes)
    }
    fn create_tag_object(
        &self,
        repository: &str,
        tag: &JournalTag,
    ) -> Result<String, OrchestratorError> {
        if !valid_tag(tag) {
            return Err(invalid());
        }
        let sha = self
            .transport
            .create_tag(repository, &tag.name, &tag.object_sha)?;
        if self.verified_tag(repository, &sha)?.0 != *tag {
            return Err(invalid());
        }
        Ok(sha)
    }
    fn create_ref(
        &self,
        repository: &str,
        reference: &str,
        tag_sha: &str,
    ) -> Result<(), OrchestratorError> {
        if !valid_reference(reference) || !valid_sha(tag_sha) {
            return Err(invalid());
        }
        let (tag, _) = self.verified_tag(repository, tag_sha)?;
        if reference != format!("refs/tags/{}", tag.name) {
            return Err(invalid());
        }
        self.transport.create_ref(repository, reference, tag_sha)
    }
    fn get_tag_object(&self, repository: &str, sha: &str) -> Result<JournalTag, OrchestratorError> {
        Ok(self.verified_tag(repository, sha)?.0)
    }
    fn get_verified_tag_bytes(
        &self,
        repository: &str,
        sha: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        Ok(self.verified_tag(repository, sha)?.1)
    }
    fn get_blob(&self, repository: &str, sha: &str) -> Result<Vec<u8>, OrchestratorError> {
        if !valid_sha(sha) {
            return Err(invalid());
        }
        let key = (repository.to_owned(), sha.to_owned());
        if let Some(value) = self.blobs.borrow().get(&key) {
            return Ok(value.clone());
        }
        let value = self.transport.get_blob(repository, sha)?;
        if crate::release_policy_admission::git_blob_sha1(&value)? != sha {
            return Err(invalid());
        }
        let mut cache = self.blobs.borrow_mut();
        if value.len() <= MAX_CACHED_BLOB_BYTES && cache.len() < MAX_CACHED_OBJECTS {
            cache.insert(key, value.clone());
        }
        Ok(value)
    }
}

fn canonical_tag(tag: &JournalTag) -> Vec<u8> {
    format!("object {}\ntype blob\ntag {}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\nvelnor-release-receipt-v1", tag.object_sha, tag.name).into_bytes()
}

fn valid_tag(tag: &JournalTag) -> bool {
    valid_reference(&format!("refs/tags/{}", tag.name))
        && tag.object_type == "blob"
        && valid_sha(&tag.object_sha)
        && tag.message == "velnor-release-receipt-v1"
}

fn valid_reference(reference: &str) -> bool {
    let Some(rest) = reference.strip_prefix("refs/tags/velnor-receipt/") else {
        return false;
    };
    let mut parts = rest.split('/');
    let run = parts
        .next()
        .and_then(|part| part.parse::<u64>().ok().filter(|id| id.to_string() == part));
    let attempt = parts
        .next()
        .and_then(|part| part.parse::<u32>().ok().filter(|id| id.to_string() == part));
    let phase = parts.next();
    run.is_some_and(|id| id > 0)
        && attempt.is_some_and(|id| id > 0)
        && phase.is_some_and(crate::crate_release_journal::valid_journal_phase)
        && parts.next().is_none()
}

fn valid_sha(sha: &str) -> bool {
    sha.len() == 40
        && sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn invalid() -> OrchestratorError {
    internal("crate_release_journal_adapter_invalid")
}
