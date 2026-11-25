// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use toml::Table;
use velnor_model::TargetKind;

use crate::toml_file::table;

pub(crate) struct Benches(Table);

impl Benches {
    pub(crate) fn read(manifest: &Path) -> Self {
        Self(
            std::fs::read(manifest)
                .ok()
                .and_then(|bytes| table(&bytes).ok())
                .unwrap_or_default(),
        )
    }

    pub(crate) fn allows(&self, kinds: &[TargetKind], name: &str) -> bool {
        let declared = kinds.iter().find_map(|kind| {
            let section = match kind {
                TargetKind::Lib | TargetKind::ProcMacro => {
                    return self.0.get("lib").and_then(bench_flag);
                }
                TargetKind::Bin => "bin",
                TargetKind::Test => "test",
                TargetKind::Bench => "bench",
                TargetKind::Example => "example",
                TargetKind::BuildScript => return None,
            };
            self.0
                .get(section)
                .and_then(toml::Value::as_array)?
                .iter()
                .find(|entry| entry.get("name").and_then(toml::Value::as_str) == Some(name))
                .and_then(bench_flag)
        });
        declared.unwrap_or_else(|| {
            kinds.iter().any(|kind| {
                matches!(
                    kind,
                    TargetKind::Lib | TargetKind::ProcMacro | TargetKind::Bin | TargetKind::Bench
                )
            })
        })
    }
}

fn bench_flag(entry: &toml::Value) -> Option<bool> {
    entry.get("bench").and_then(toml::Value::as_bool)
}
