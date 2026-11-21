// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

fn help(command: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args([command, "--help"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).replace(['\n', ' '], " ")
}

#[test]
fn only_mbx_store_claims_to_write_the_out_dir_setting() {
    let store = help("mbx-store");
    assert!(
        store.contains("Always appends MBX_SHARE_OUT_DIR=0"),
        "{store}"
    );
    let seed = help("mbx-seed");
    assert!(
        !seed.contains("Always appends MBX_SHARE_OUT_DIR=0"),
        "{seed}"
    );
    assert!(
        seed.contains("It never writes MBX_SHARE_OUT_DIR; mbx-store does"),
        "{seed}"
    );
}
