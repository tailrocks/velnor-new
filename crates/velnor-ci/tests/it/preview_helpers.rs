// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::{Command, Output};

const ID: &str = "VELNOR_EVIDENCE_ARTIFACT_ID";

fn validate(id: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command.arg("validate-artifact-id").env_remove(ID);
    if let Some(id) = id {
        command.env(ID, id);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn canonical_ids_up_to_the_javascript_safe_integer_pass() {
    for id in ["1", "42", "9007199254740991", "999999999999999"] {
        let output = validate(Some(id));
        assert!(output.status.success(), "{id}: {output:?}");
    }
}

#[test]
fn every_other_id_fails_closed() {
    for id in [
        "",
        "0",
        "01",
        "-1",
        "+1",
        "1.5",
        "12a",
        " 1",
        "1 ",
        "1\n",
        "9007199254740992",
        "10000000000000000",
        "99999999999999999",
        "\u{661}",
    ] {
        let output = validate(Some(id));
        assert_eq!(output.status.code(), Some(1), "{id:?}: {output:?}");
    }
    assert_eq!(validate(None).status.code(), Some(1));
}
