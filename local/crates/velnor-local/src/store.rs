// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use velnor_cache_rules::{
    Access, AlreadyExists, Entry, EntryId, Scope, Timestamp, evict, lookup, reserve,
};

pub const RESERVATION_SECONDS: u64 = 60 * 60;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Index {
    pub next_id: u64,
    pub records: Vec<Record>,
    pub pull_request_bases: BTreeMap<u64, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Record {
    pub id: u64,
    pub blob: String,
    pub repository: String,
    pub scope: String,
    pub key: String,
    pub version: String,
    pub size_bytes: u64,
    pub created: u64,
    pub last_access: u64,
    pub committed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lookup<'a> {
    pub repository: &'a str,
    pub access: &'a Access,
    pub key: &'a str,
    pub version: &'a str,
}

impl Record {
    fn entry(&self) -> Entry {
        Entry {
            scope: Scope(self.scope.clone()),
            key: self.key.clone(),
            version: self.version.clone(),
            size_bytes: self.size_bytes,
            created: Timestamp(self.created),
            last_access: Timestamp(self.last_access),
        }
    }

    fn is(&self, repository: &str, id: &EntryId) -> bool {
        self.repository == repository
            && self.scope == id.scope.0
            && self.key == id.key
            && self.version == id.version
    }
}

impl Lookup<'_> {
    fn id(&self) -> EntryId {
        EntryId {
            scope: self.access.write.clone(),
            key: self.key.to_owned(),
            version: self.version.to_owned(),
        }
    }
}

impl Index {
    fn entries(&self, repository: &str, committed_only: bool, now: u64) -> Vec<Entry> {
        let reserving = |record: &Record| {
            !committed_only && now.saturating_sub(record.created) <= RESERVATION_SECONDS
        };
        self.records
            .iter()
            .filter(|record| {
                record.repository == repository && (record.committed || reserving(record))
            })
            .map(Record::entry)
            .collect()
    }

    pub fn restore(
        &mut self,
        at: &Lookup<'_>,
        restore_keys: &[String],
        now: u64,
    ) -> Option<Record> {
        let mut entries = self.entries(at.repository, true, now);
        let expired = evict(&entries, Timestamp(now), u64::MAX);
        entries.retain(|entry| !expired.contains(&entry.id()));
        let found = lookup(&entries, at.access, at.key, restore_keys, at.version)?.id();
        let record = self
            .records
            .iter_mut()
            .find(|record| record.committed && record.is(at.repository, &found))?;
        record.last_access = now;
        Some(record.clone())
    }

    pub fn create(
        &mut self,
        at: &Lookup<'_>,
        blob: String,
        now: u64,
    ) -> Result<Record, AlreadyExists> {
        reserve(
            &self.entries(at.repository, false, now),
            at.access,
            at.key,
            at.version,
        )?;
        let id = at.id();
        if let Some(stale) = self
            .records
            .iter_mut()
            .find(|record| !record.committed && record.is(at.repository, &id))
        {
            stale.created = now;
            stale.last_access = now;
            return Ok(stale.clone());
        }
        self.next_id += 1;
        let record = Record {
            id: self.next_id,
            blob,
            repository: at.repository.to_owned(),
            scope: at.access.write.0.clone(),
            key: at.key.to_owned(),
            version: at.version.to_owned(),
            size_bytes: 0,
            created: now,
            last_access: now,
            committed: false,
        };
        self.records.push(record.clone());
        Ok(record)
    }

    #[must_use]
    pub fn pending(&self, at: &Lookup<'_>) -> Option<&Record> {
        let id = at.id();
        self.records
            .iter()
            .find(|record| !record.committed && record.is(at.repository, &id))
    }

    #[must_use]
    pub fn blob(&self, blob: &str, committed: bool) -> Option<&Record> {
        self.records
            .iter()
            .find(|record| record.blob == blob && record.committed == committed)
    }

    pub fn commit(&mut self, id: u64, size_bytes: u64, now: u64, limit_bytes: u64) -> Vec<Record> {
        let Some(record) = self.records.iter_mut().find(|record| record.id == id) else {
            return Vec::new();
        };
        record.committed = true;
        record.size_bytes = size_bytes;
        record.last_access = now;
        let repository = record.repository.clone();
        let evicted = evict(
            &self.entries(&repository, false, now),
            Timestamp(now),
            limit_bytes,
        );
        let (gone, kept) = std::mem::take(&mut self.records)
            .into_iter()
            .partition(|record| evicted.iter().any(|id| record.is(&repository, id)));
        self.records = kept;
        gone
    }
}
