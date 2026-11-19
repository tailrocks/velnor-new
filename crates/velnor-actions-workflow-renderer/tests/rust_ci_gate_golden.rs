// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The Rust CI gate job name has one source: the generated `rust-ci.yml` of
//! the velnor-new sample (`jobs.required.name`). The renderer's gate constant
//! and the image authority's identity constant must equal it, so drift in any
//! of the three fails a test instead of blocking every release.

use serde_json::Value;

const GOLDEN: &str =
    include_str!("../../../generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml");
const GATE_SOURCE: &str = include_str!("../src/schema2_rust_ci_gate.rs");
const IDENTITY_SOURCE: &str = include_str!("../src/job_image_authority_identity.py");

fn quoted_after<'a>(source: &'a str, prefix: &str) -> &'a str {
    let rest = source
        .split_once(prefix)
        .map_or("", |(_, rest)| rest.trim_start());
    let rest = rest.strip_prefix('"').unwrap_or("");
    rest.split('"').next().unwrap_or("")
}

/// The gate job's display name as the generator writes it.
pub(crate) fn gate_job_name() -> String {
    let workflow: Value = serde_saphyr::from_str(GOLDEN).unwrap_or(Value::Null);
    let name = workflow["jobs"]["required"]["name"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        !name.is_empty(),
        "the velnor-new rust-ci.yml golden has no jobs.required.name"
    );
    assert_eq!(
        quoted_after(GATE_SOURCE, "const REQUIRED_JOB_NAME: &str ="),
        name,
        "schema2_rust_ci_gate.rs REQUIRED_JOB_NAME drifted from the generated gate job name"
    );
    assert_eq!(
        quoted_after(IDENTITY_SOURCE, "RUST_REQUIRED_JOB ="),
        name,
        "job_image_authority_identity.py RUST_REQUIRED_JOB drifted from the generated gate job name"
    );
    name
}

#[test]
fn gate_job_name_is_one_value_across_generator_gate_and_identity() {
    assert_eq!(gate_job_name(), "rust-required");
}
