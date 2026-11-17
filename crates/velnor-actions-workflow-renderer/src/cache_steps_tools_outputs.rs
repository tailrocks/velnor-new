// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::yaml::Yaml;

pub(super) fn action_outputs() -> Yaml {
    let mut entries = Vec::with_capacity(5);
    for (name, description, value) in [
        (
            "cache-hit",
            "Exact-key hit of the restored V2 tools entry.",
            "${{ steps.restore.outputs.cache-hit }}",
        ),
        (
            "origin-bytes",
            "Origin bytes downloaded while re-verifying restored Mise installs; empty when unmeasured.",
            "${{ steps.verify.outputs.origin-bytes }}",
        ),
        (
            "origin-fetches",
            "Origin archive downloads while re-verifying restored Mise installs.",
            "${{ steps.verify.outputs.origin-fetches }}",
        ),
        (
            "reused-archives",
            "Verified local tool archives reused without an origin download.",
            "${{ steps.verify.outputs.reused-archives }}",
        ),
        (
            "verified-files",
            "Restored install files verified against trusted archives.",
            "${{ steps.verify.outputs.verified-files }}",
        ),
    ] {
        entries.push((
            name.to_owned(),
            Yaml::Map(vec![
                ("description".to_owned(), Yaml::str(description)),
                ("value".to_owned(), Yaml::str(value)),
            ]),
        ));
    }
    Yaml::Map(entries)
}
