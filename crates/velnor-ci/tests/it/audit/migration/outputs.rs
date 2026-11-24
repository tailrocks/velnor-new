// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

pub(in crate::audit) struct RunOutput {
    pub(in crate::audit) unit: String,
    pub(in crate::audit) stdout: PathBuf,
}

impl RunOutput {
    pub(in crate::audit) fn receipt(&self) -> PathBuf {
        let mut receipt = self.stdout.clone().into_os_string();
        receipt.push(".velnor-inputs.json");
        receipt.into()
    }
}

fn is_hash(text: &str) -> bool {
    text.len() == 16 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn entries(directory: &Path) -> Vec<(String, PathBuf)> {
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .map(|entry| entry.unwrap_or_else(|e| unreachable!("{e}")))
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
            )
        })
        .collect()
}

pub(in crate::audit) fn run_outputs(build: &Path, package: &str) -> Vec<RunOutput> {
    let nested = entries(&build.join(package))
        .into_iter()
        .map(|(hash, path)| (hash, path.join("run/stdout")));
    let flat = entries(build).into_iter().filter_map(|(folder, path)| {
        let hash = folder.strip_prefix(package)?.strip_prefix('-')?;
        is_hash(hash).then(|| (hash.to_owned(), path.join("output")))
    });
    nested
        .chain(flat)
        .filter(|(_, stdout)| stdout.is_file())
        .map(|(unit, stdout)| RunOutput { unit, stdout })
        .collect()
}

#[test]
fn run_outputs_are_found_in_the_layouts_of_cargo_1100_and_before() {
    use crate::audit::unit;
    let repo = crate::io::Repo::new("audit-run-outputs");
    unit(&repo, "new/build/lib/f000000000000001/run/stdout", "");
    unit(&repo, "new/build/lib/f000000000000002/out/lib.d", "");
    unit(&repo, "old/build/lib-f000000000000003/output", "");
    unit(&repo, "old/build/lib-f000000000000004/lib.d", "");
    unit(&repo, "old/build/lib-build-f000000000000005/output", "");
    let units = |build: &str| -> Vec<String> {
        run_outputs(&repo.root().join(build), "lib")
            .into_iter()
            .map(|run| run.unit)
            .collect()
    };
    assert_eq!(units("new/build"), ["f000000000000001"]);
    assert_eq!(units("old/build"), ["f000000000000003"]);
}
