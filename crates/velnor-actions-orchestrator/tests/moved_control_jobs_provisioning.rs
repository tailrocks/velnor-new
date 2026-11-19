// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The control jobs moved to the scale set run on an image that ships no
//! tools, unlike the hosted image. Each moved job must render the tool
//! provisioning the scale-set Rust lanes use, or it fails on a missing binary.

use std::error::Error;
use std::fs;
use std::path::Path;

use serde_json::Value;

const SCALE_SET: &str = "ubuntu-26.04-scale-set";

struct Moved {
    id: &'static str,
    /// Step names the job must carry, in order.
    steps: &'static [&'static str],
    /// A step whose command must install and run the project Rust toolchain.
    rust_in: Option<&'static str>,
}

const MOVED: [Moved; 8] = [
    Moved {
        id: "plan",
        steps: &["Resolve project tool versions", "Prepare pinned tools"],
        rust_in: None,
    },
    Moved {
        id: "alint",
        steps: &["Run Alint"],
        rust_in: None,
    },
    Moved {
        id: "cargo-deny",
        steps: &[
            "Setup Mise",
            "Resolve project tool versions",
            "Run cargo-deny",
        ],
        rust_in: Some("Run cargo-deny"),
    },
    Moved {
        id: "cargo-machete",
        steps: &["Setup Mise", "Prepare pinned tools", "Run cargo-machete"],
        rust_in: None,
    },
    Moved {
        id: "jactionlint",
        steps: &[
            "Setup Mise",
            "Resolve project tool versions",
            "Prepare pinned tools",
            "Verify jactionlint",
            "Run jactionlint",
        ],
        rust_in: None,
    },
    Moved {
        id: "ledger-cli",
        steps: &[
            "Setup Mise",
            "Resolve project tool versions",
            "Prepare pinned tools",
        ],
        rust_in: None,
    },
    Moved {
        id: "required",
        steps: &["Setup Mise", "Prepare pinned tools"],
        rust_in: None,
    },
    Moved {
        id: "required-gate",
        steps: &["Fail unless the verdict passed"],
        rust_in: None,
    },
];

fn workflow() -> Result<Value, Box<dyn Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml");
    Ok(serde_saphyr::from_str_with_options(
        &fs::read_to_string(path)?,
        serde_saphyr::options! { budget: serde_saphyr::budget! { enforce_alias_anchor_ratio: false } },
    )?)
}

fn names(job: &Value) -> Vec<String> {
    job["steps"]
        .as_array()
        .map(|steps| {
            steps
                .iter()
                .filter_map(|step| step["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn in_order(have: &[String], want: &[&str]) -> bool {
    let mut at = 0;
    for name in have {
        if at < want.len() && name == want[at] {
            at += 1;
        }
    }
    at == want.len()
}

#[test]
fn every_moved_control_job_renders_its_tool_provisioning() -> Result<(), Box<dyn Error>> {
    let workflow = workflow()?;
    for moved in &MOVED {
        let job = &workflow["jobs"][moved.id];
        assert!(
            job["runs-on"].to_string().contains(SCALE_SET),
            "{} must run on the scale set",
            moved.id
        );
        let have = names(job);
        assert!(
            in_order(&have, moved.steps),
            "{} must render {:?} in order, has {have:?}",
            moved.id,
            moved.steps
        );
    }
    Ok(())
}

#[test]
fn a_moved_cargo_job_installs_and_runs_the_project_rust_toolchain() -> Result<(), Box<dyn Error>> {
    let workflow = workflow()?;
    for moved in MOVED.iter().filter(|moved| moved.rust_in.is_some()) {
        let step = workflow["jobs"][moved.id]["steps"]
            .as_array()
            .and_then(|steps| {
                steps
                    .iter()
                    .find(|step| step["name"].as_str() == moved.rust_in)
            })
            .ok_or("missing the cargo step")?;
        let run = step["run"].as_str().ok_or("cargo step has no run")?;
        assert!(
            run.contains("rust[profile=minimal]@"),
            "{}: no install",
            moved.id
        );
        assert!(
            run.contains("rust@"),
            "{}: cargo is not provisioned",
            moved.id
        );
        assert!(
            run.contains("${VELNOR_TOOL_RUST}"),
            "{}: not project-pinned",
            moved.id
        );
    }
    Ok(())
}
