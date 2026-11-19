// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::verdict::{PackageCounts, encode_package_counts, parse_package_counts};
use std::collections::BTreeMap;

#[test]
fn a_package_map_larger_than_32_kib_round_trips() {
    let packages: BTreeMap<String, PackageCounts> = (0..2000)
        .map(|index| {
            (
                format!("package-with-a-long-name-{index:05}"),
                PackageCounts { run: 1, listed: 2 },
            )
        })
        .collect();
    let encoded = encode_package_counts(&packages).expect("encodes");
    assert!(encoded.len() > 32_768, "{}", encoded.len());
    assert_eq!(parse_package_counts(&encoded), Some(packages));
}
