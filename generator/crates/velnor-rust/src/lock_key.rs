// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::sha256::sha256_hex;

const RAW: &[u8] = b"velnor-lock-key raw\n";
const MEMBERS: &[u8] = b"velnor-lock-key members\n";
const ABSENT: &[u8] = b"velnor-lock-key absent\n";

#[must_use]
pub fn lock_key(lock: Option<&[u8]>, members: &BTreeSet<String>) -> String {
    let Some(bytes) = lock else {
        return sha256_hex(ABSENT);
    };
    match without_member_versions(bytes, members) {
        Some(text) => sha256_hex(&[MEMBERS, text.as_bytes()].concat()),
        None => sha256_hex(&[RAW, bytes].concat()),
    }
}

fn without_member_versions(bytes: &[u8], members: &BTreeSet<String>) -> Option<String> {
    let mut document: toml::Table = toml::from_str(std::str::from_utf8(bytes).ok()?).ok()?;
    let packages = document.get_mut("package")?.as_array_mut()?;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for package in packages.iter() {
        let name = package.as_table()?.get("name")?.as_str()?;
        *counts.entry(name.to_owned()).or_default() += 1;
    }
    for package in packages.iter_mut() {
        let table = package.as_table_mut()?;
        let name = table.get("name")?.as_str()?.to_owned();
        let unique_member = members.contains(&name) && counts.get(&name) == Some(&1);
        if unique_member && !table.contains_key("source") {
            table.remove("version");
        }
    }
    toml::to_string(&document).ok()
}
