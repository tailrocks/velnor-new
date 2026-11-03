// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) struct CacheService {
    entries: Vec<(String, u64)>,
}

impl CacheService {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub(crate) fn save(&mut self, key: &str, created_at: u64) {
        self.entries.push((key.to_owned(), created_at));
    }

    pub(crate) fn restore(&self, key: &str, restore_keys: &[String]) -> Option<String> {
        if let Some((exact, _)) = self.entries.iter().find(|(entry, _)| entry == key) {
            return Some(exact.clone());
        }
        restore_keys.iter().find_map(|prefix| {
            self.entries
                .iter()
                .filter(|(entry, _)| entry.starts_with(prefix.as_str()))
                .max_by_key(|(_, created_at)| *created_at)
                .map(|(entry, _)| entry.clone())
        })
    }
}
