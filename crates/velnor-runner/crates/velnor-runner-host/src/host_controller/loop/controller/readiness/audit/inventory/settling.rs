// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::Mutex,
    time::{Duration, Instant},
};

use sha2::Digest;

use super::InventoryEntry;
use crate::error::HostError;

pub(super) const SETTLING_BOUND: Duration = Duration::from_secs(600);

type PairKey = (String, i64);

#[derive(Default)]
pub(in crate::host_controller::loop_driver::controller) struct SettlingAges {
    first_seen: Mutex<BTreeMap<PairKey, Instant>>,
}

impl SettlingAges {
    pub(super) fn observe(&self, settling: &[PairKey], now: Instant) -> Result<(), HostError> {
        let mut first_seen = self.first_seen.lock().map_err(|_| HostError::Journal)?;
        first_seen.retain(|key, _| settling.contains(key));
        for key in settling {
            first_seen.entry(key.clone()).or_insert(now);
        }
        for ((connection_id, reservation_id), since) in first_seen.iter() {
            let age = now.saturating_duration_since(*since);
            if age > SETTLING_BOUND {
                eprintln!(
                    "readiness=settling_exceeded connection_id={connection_id} reservation_id={reservation_id} seconds={}",
                    age.as_secs()
                );
                return Err(HostError::SettlingTimeout);
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Inventory {
    entries: Vec<InventoryEntry>,
    settling: Vec<PairKey>,
}

impl Inventory {
    pub(super) fn settle(&mut self, connection_id: &str, reservation_id: i64) {
        self.settling
            .push((connection_id.to_owned(), reservation_id));
    }

    pub(super) fn admit(&mut self, remote_present: bool, entry: InventoryEntry) {
        if remote_present {
            self.entries.push(entry);
        } else {
            self.settle(&entry.connection_id, entry.reservation_id);
        }
    }

    pub(super) fn finish(
        self,
        ages: &SettlingAges,
        now: Instant,
    ) -> Result<Option<String>, HostError> {
        ages.observe(&self.settling, now)?;
        if !self.settling.is_empty() {
            return Ok(None);
        }
        let bytes = serde_json::to_vec(&self.entries).map_err(|_| HostError::Identity)?;
        let mut digest = String::with_capacity(64);
        for byte in sha2::Sha256::digest(bytes) {
            write!(digest, "{byte:02x}").map_err(|_| HostError::Identity)?;
        }
        Ok(Some(digest))
    }
}
