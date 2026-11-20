// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use std::fs;
use velnor_ci::mbx_declared::declared_in;
use velnor_ci::report::{LaneMetrics, check, waste};

const VIOLATION: &str = "compiled without mbx";

fn metrics(value: Value) -> LaneMetrics {
    serde_json::from_value(value).unwrap_or_else(|error| unreachable!("{error}"))
}

fn lane(artifacts: &Value, mbx: &Value) -> Value {
    let mut value = super::report::lane();
    value["caches"] = json!([]);
    value["invocations"][0]["compilations"] = json!([]);
    value["invocations"][0]["artifacts"] = artifacts.clone();
    value["mbx"] = mbx.clone();
    value
}

fn artifact(fresh: bool) -> Value {
    json!({
        "package_id":"path+file:///checkout/crates/mine#mine@0.1.0",
        "manifest_path":"crates/mine/Cargo.toml",
        "target":{"name":"mine","kind":["lib"],"crate_types":["lib"],"src_path":"crates/mine/src/lib.rs"},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
        "features":[],"filenames":["target/debug/deps/libmine-abc.rlib"],"fresh":fresh
    })
}

fn session() -> Value {
    json!({"hits":3,"misses":1,"not_looked_up":0})
}

#[test]
fn a_lane_that_compiled_units_without_an_mbx_session_is_a_violation_when_mbx_is_declared() {
    for (name, artifacts, mbx, expected) in [
        (
            "declared, compiled, no session",
            json!([artifact(false)]),
            json!({"declared":true,"store_primed":false,"sessions":[]}),
            true,
        ),
        (
            "declared, compiled, with a session",
            json!([artifact(false)]),
            json!({"declared":true,"store_primed":false,"sessions":[session()]}),
            false,
        ),
        (
            "not declared, compiled, no session",
            json!([artifact(false)]),
            json!({"store_primed":false,"sessions":[]}),
            false,
        ),
        (
            "declared, nothing compiled",
            json!([]),
            json!({"declared":true,"store_primed":false,"sessions":[]}),
            false,
        ),
        (
            "declared, only fresh units",
            json!([artifact(true)]),
            json!({"declared":true,"store_primed":false,"sessions":[]}),
            false,
        ),
    ] {
        let found = waste(&metrics(lane(&artifacts, &mbx)));
        assert_eq!(
            found.iter().any(|line| line.contains(VIOLATION)),
            expected,
            "{name}: {found:?}"
        );
    }
}

#[test]
fn the_check_fails_on_a_lane_that_compiled_without_mbx() {
    let value = lane(
        &json!([artifact(false)]),
        &json!({"declared":true,"store_primed":false,"sessions":[]}),
    );
    let error = check(&[metrics(value)])
        .err()
        .map(|error| error.to_string());
    assert!(
        error.is_some_and(|text| text.contains(VIOLATION)),
        "the check must fail"
    );
}

#[test]
fn declaration_reads_the_owner_layout_and_nothing_else() -> Result<(), Box<dyn std::error::Error>> {
    for (name, text, expected) in [
        (
            "rust option",
            "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\n",
            true,
        ),
        (
            "wrapper",
            "[wrappers.cargo]\ncommand = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n",
            true,
        ),
        (
            "rust option false",
            "[tools]\nrust = { version = \"1.99.1\", mr_boxington = false }\n",
            false,
        ),
        (
            "a pin alone",
            "[tools]\nmr-boxington = \"v1.30.0\"\nrust = \"1.99.1\"\n",
            false,
        ),
        (
            "another wrapper",
            "[wrappers.cargo]\ncommand = \"sccache-wrap\"\n",
            false,
        ),
        ("unreadable", "[tools\n", false),
    ] {
        let directory = std::env::temp_dir().join(format!(
            "velnor-mbx-declared-{}-{}",
            std::process::id(),
            name.replace([' ', ','], "-")
        ));
        fs::create_dir_all(&directory)?;
        fs::write(directory.join("mise.toml"), text)?;
        let found = declared_in(&directory);
        fs::remove_dir_all(&directory)?;
        assert_eq!(found, expected, "{name}");
    }
    let empty =
        std::env::temp_dir().join(format!("velnor-mbx-declared-none-{}", std::process::id()));
    fs::create_dir_all(&empty)?;
    let found = declared_in(&empty);
    fs::remove_dir_all(&empty)?;
    assert!(!found, "no mise config declares nothing");
    Ok(())
}

#[test]
fn declaration_never_drops_a_file_when_two_files_set_the_mbx_keys()
-> Result<(), Box<dyn std::error::Error>> {
    let standard = "[wrappers.cargo]\ncommand = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n";
    for (name, files, expected) in [
        (
            "standard wrapper and another wrapper",
            [
                ("mise.toml", standard),
                (".mise.toml", "[wrappers.cargo]\ncommand = \"sccache\"\n"),
            ],
            true,
        ),
        (
            "another wrapper and standard wrapper",
            [
                ("mise.toml", "[wrappers.cargo]\ncommand = \"sccache\"\n"),
                (".mise.toml", standard),
            ],
            true,
        ),
        (
            "option in the hidden file only",
            [
                ("mise.toml", "[tools]\nrust = \"1.99.1\"\n"),
                (
                    ".mise.toml",
                    "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\n",
                ),
            ],
            true,
        ),
        (
            "unrelated files",
            [
                ("mise.toml", "[tools]\nrust = \"1.99.1\"\n"),
                (".mise.toml", "[tools]\nnode = \"24\"\n"),
            ],
            false,
        ),
    ] {
        let directory = std::env::temp_dir().join(format!(
            "velnor-mbx-split-{}-{}",
            std::process::id(),
            name.replace(' ', "-")
        ));
        fs::create_dir_all(&directory)?;
        for (file, text) in files {
            fs::write(directory.join(file), text)?;
        }
        let found = declared_in(&directory);
        fs::remove_dir_all(&directory)?;
        assert_eq!(found, expected, "{name}");
    }
    Ok(())
}
