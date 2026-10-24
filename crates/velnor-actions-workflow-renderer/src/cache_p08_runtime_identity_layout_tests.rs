use std::{collections::BTreeMap, error::Error};

use velnor_actions_contract::StepKind;

use super::payload;

#[test]
fn generated_identity_actions_bind_each_supported_hosted_lane() -> Result<(), Box<dyn Error>> {
    for (lane, path, uses) in [
        (
            "ubuntu-22.04",
            ".github/actions/u22/action.yml",
            "./.github/actions/u22",
        ),
        (
            "ubuntu-24.04",
            ".github/actions/u24/action.yml",
            "./.github/actions/u24",
        ),
        (
            "ubuntu-26.04",
            ".github/actions/u26/action.yml",
            "./.github/actions/u26",
        ),
    ] {
        let file = super::super::action_file(lane, env!("CARGO_PKG_VERSION"))?;
        assert_eq!(file.path, path);
        assert!(file.bytes.contains(&format!("VELNOR_CACHE_LANE: {lane}")));
        let input = crate::cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;
        assert!(file.bytes.contains(&format!("  {input}:\n")));
        assert!(file.bytes.contains(&format!(
            "VELNOR_CACHE_STATIC_DIGEST: ${{{{ inputs.{input} }}}}"
        )));
        assert_eq!(super::super::action_uses(lane), Some(uses));
    }
    Ok(())
}

#[test]
fn scale_set_identity_action_requires_and_forwards_its_image_source_pin()
-> Result<(), Box<dyn Error>> {
    let lane = "scale-set:velnor+ubuntu-26.04-scale-set";
    let input = super::super::TOOLS_CACHE_SCALE_SET_IMAGE_SOURCE_SHA_INPUT;
    let file = super::super::action_file(lane, env!("CARGO_PKG_VERSION"))?;
    assert_eq!(file.path, ".github/actions/scale-set/action.yml");
    assert!(file.bytes.contains(&format!("  {input}:\n")));
    assert!(file.bytes.contains(&format!(
        "VELNOR_CACHE_SCALE_SET_IMAGE_SOURCE_SHA: ${{{{ inputs.{input} }}}}"
    )));
    assert_eq!(
        super::super::action_uses(lane),
        Some("./.github/actions/scale-set")
    );
    assert!(super::super::is_supported_lane(
        lane,
        "x86_64-unknown-linux-gnu"
    ));
    Ok(())
}

#[test]
fn identity_step_binds_fixed_tool_home_environment() {
    let payload = payload("ubuntu-26.04");
    let step = payload.runtime_prelude_step().expect("identity prelude");
    let StepKind::Action { uses, with, env } = step.kind else {
        panic!("identity is a composite action step");
    };
    assert_eq!(
        Some(uses.as_str()),
        crate::cache_p08::runtime_prelude_action_uses("ubuntu-26.04")
    );
    assert_eq!(env.len(), 0);
    assert_eq!(
        with.get(crate::cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
            .map(String::as_str),
        Some(payload.static_digest())
    );
    let inner = super::super::inner_step("ubuntu-26.04").expect("composite identity shell step");
    let StepKind::Shell { env, .. } = inner.kind else {
        panic!("composite identity body is a shell step");
    };
    let homes = BTreeMap::from([
        ("MISE_RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
        ("RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
        ("MISE_CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
        ("CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
    ]);
    for (key, value) in homes {
        assert_eq!(env.get(key).map(String::as_str), Some(value));
    }
    assert_eq!(step.name, crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME);
}
