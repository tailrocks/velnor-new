// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{
    Access, AlreadyExists, Entry, EntryId, Event, Scope, Timestamp, access_for, lookup, reserve,
};

pub(crate) const VERSION: &str = "v1";
pub(crate) const MAIN: &str = "refs/heads/main";
pub(crate) const PR7: &str = "refs/pull/7/merge";

pub(crate) fn push(branch: &str) -> Event {
    Event::Push {
        branch: branch.to_owned(),
    }
}

pub(crate) fn tag(name: &str) -> Event {
    Event::Tag {
        name: name.to_owned(),
    }
}

pub(crate) fn pr(number: u64, base_branch: &str) -> Event {
    Event::PullRequest {
        number,
        base_branch: base_branch.to_owned(),
    }
}

pub(crate) fn queue(queue_branch: &str) -> Event {
    Event::MergeGroup {
        queue_branch: queue_branch.to_owned(),
    }
}

pub(crate) fn access(event: &Event) -> Access {
    access_for(event, "main")
}

pub(crate) fn scopes(names: &[&str]) -> Vec<Scope> {
    names.iter().map(|name| Scope((*name).to_owned())).collect()
}

pub(crate) fn id(scope: &str, key: &str, version: &str) -> EntryId {
    EntryId {
        scope: Scope(scope.to_owned()),
        key: key.to_owned(),
        version: version.to_owned(),
    }
}

pub(crate) fn entry(scope: &str, key: &str, version: &str, created: u64) -> Entry {
    Entry {
        scope: Scope(scope.to_owned()),
        key: key.to_owned(),
        version: version.to_owned(),
        size_bytes: 1,
        created: Timestamp(created),
        last_access: Timestamp(created),
    }
}

pub(crate) fn sized(key: &str, size_bytes: u64, last_access: u64) -> Entry {
    Entry {
        size_bytes,
        ..entry(MAIN, key, VERSION, last_access)
    }
}

pub(crate) fn found(
    entries: &[Entry],
    event: &Event,
    key: &str,
    restore_keys: &[&str],
    version: &str,
) -> Option<(String, String)> {
    let restore_keys: Vec<String> = restore_keys.iter().map(|key| (*key).to_owned()).collect();
    lookup(entries, &access(event), key, &restore_keys, version)
        .map(|hit| (hit.scope.0.clone(), hit.key.clone()))
}

pub(crate) struct Repository {
    entries: Vec<Entry>,
    clock: u64,
}

impl Repository {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            clock: 0,
        }
    }

    pub(crate) fn restore(
        &self,
        event: &Event,
        key: &str,
        restore_keys: &[&str],
    ) -> Option<(String, String)> {
        found(&self.entries, event, key, restore_keys, VERSION)
    }

    pub(crate) fn save(&mut self, event: &Event, key: &str) -> Result<(), AlreadyExists> {
        self.clock += 1;
        let access = access(event);
        reserve(&self.entries, &access, key, VERSION)?;
        self.entries
            .push(entry(&access.write.0, key, VERSION, self.clock));
        Ok(())
    }
}
