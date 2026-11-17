// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use velnor_actions_workflow_renderer::{
    MiseSetup,
    cache_p08::{ToolsCacheInputs, ToolsCachePayload},
};

fn payload(rust: Option<&str>) -> Result<ToolsCachePayload, Box<dyn Error>> {
    let mise = MiseSetup {
        uses: "jdx/mise-action@f5be966e4a917d54244728e6a5fdcd1d93a35e6c".to_owned(),
        version: "2026.11.1".to_owned(),
        sha256: "7129dbab31d698f9028db9374f7ba7987232b0426f837dcbce437130a61415f9".to_owned(),
    };
    let mut tools = vec![
        "jactionlint@2.4.0".to_owned(),
        "shellcheck@0.11.0".to_owned(),
        "gh@2.103.3".to_owned(),
    ];
    tools.extend(rust.map(str::to_owned));
    Ok(ToolsCachePayload::new(ToolsCacheInputs {
        runs_on: "ubuntu-26.04",
        target: "x86_64-unknown-linux-gnu",
        mise_setup: &mise,
        tool_specs: &tools,
        rustup_toolchain: rust.map(|_| "1.99.1"),
        rustup_components: &[],
    })?)
}

#[test]
fn archive_material_rollout_rejects_previous_nonrust_exact_identity() -> Result<(), Box<dyn Error>>
{
    let native = payload(None)?;
    assert_ne!(
        native.identity_input(),
        "fe03f65925895cf87ba800869a199725c602f8c5b8bfd942481398672da7ed8e",
        "old exact entries still suppress archive production"
    );
    let step = native.runtime_prelude_step()?;
    let velnor_actions_contract::StepKind::Action { with, .. } = step.kind else {
        return Err("identity is not generated action".into());
    };
    assert_eq!(with.get("d"), Some(&native.identity_input()));
    for (rust, baseline) in [
        (
            "rust[profile=minimal]@1.99.1",
            "e948c4d89f85b20d612360e458649d393b1c466fb1017f27a687b4927acdffd6",
        ),
        (
            "rust@1.99.1",
            "daaa717006c54da9ddf53f5775553dd4e9808734e058466771c3834411c98eb5",
        ),
    ] {
        assert_ne!(payload(Some(rust))?.identity_input(), baseline);
    }
    Ok(())
}
