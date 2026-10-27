// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn reject_with(
    label: &str,
    with: BTreeMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let jobs = BTreeMap::from([("worker".to_owned(), job_with(vec![setup_step(with)]))]);
    if check_mbx_gating(&jobs, &BTreeMap::new()).is_ok() {
        return Err(fail(&format!("explicit mbx selection accepted: {label}")));
    }
    Ok(())
}

pub(crate) fn check_rejects_explicit_selection() -> Result<(), Box<dyn Error>> {
    reject_with(
        "tool_versions",
        BTreeMap::from([
            ("install".to_owned(), "false".to_owned()),
            (
                "tool_versions".to_owned(),
                "gh@2.103.2 mr-boxington@1.2.3".to_owned(),
            ),
        ]),
    )?;
    reject_with(
        "bare selector",
        BTreeMap::from([("tool_versions".to_owned(), "mr-boxington".to_owned())]),
    )?;
    let shell = Step {
        name: "Select".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec![
                "sh".to_owned(),
                "-c".to_owned(),
                "mise --no-config --no-env --no-hooks exec mr-boxington@1.2.3 -- echo hi"
                    .to_owned(),
            ],
            env: BTreeMap::new(),
        },
    };
    let jobs = BTreeMap::from([("worker".to_owned(), job_with(vec![shell]))]);
    if check_mbx_gating(&jobs, &BTreeMap::new()).is_ok() {
        return Err(fail("shell mbx selection accepted"));
    }
    let action = Step {
        name: "Build".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: mbx_uses(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    };
    let jobs = BTreeMap::from([("worker".to_owned(), job_with(vec![action]))]);
    let drivers = BTreeMap::from([("worker".to_owned(), CompileDriver::Cargo)]);
    if check_mbx_gating(&jobs, &drivers).is_ok() {
        return Err(fail("cargo driver kept its mbx action"));
    }
    if check_mbx_gating(
        &jobs,
        &BTreeMap::from([("ghost".to_owned(), CompileDriver::Mbx)]),
    )
    .is_ok()
    {
        return Err(fail("unknown gating job accepted"));
    }
    Ok(())
}

pub(crate) fn check_rejects_bad_transport_inputs() -> Result<(), Box<dyn Error>> {
    if mbx_steps_for_driver(
        &mbx_uses(),
        CompileDriver::Mbx,
        "1.2.3",
        "not-a-toolchain",
        driver_env()?,
        MbxTransport::SeedRead,
        None,
    )
    .is_ok()
    {
        return Err(fail("state transport accepted its bad toolchain"));
    }
    if mbx_steps_for_driver(
        &mbx_uses(),
        CompileDriver::Mbx,
        "not-a-version",
        "1.89.0",
        driver_env()?,
        MbxTransport::SeedRead,
        None,
    )
    .is_ok()
    {
        return Err(fail("state transport accepted its bad tool version"));
    }
    if cache_action_step(
        true,
        "actions/cache/restore",
        "mbx-bundle",
        MBX_BUNDLE_KEY_EXPR,
        &[MBX_BUNDLE_RESTORE_KEYS_EXPR.to_owned()],
        &[COMPLETE_STATE_TRANSPORT_EXPR.to_owned()],
    )
    .is_ok()
    {
        return Err(fail("state transport accepted its unpinned restore action"));
    }
    if cache_action_step(
        true,
        TOOLS_RESTORE_ACTION_USES,
        "mbx-bundle",
        MBX_BUNDLE_KEY_EXPR,
        &[MBX_BUNDLE_RESTORE_KEYS_EXPR.to_owned()],
        &[MBX_BUNDLE_PATH_EXPR.to_owned()],
    )
    .is_ok()
    {
        return Err(fail("state transport accepted its stale bundle path"));
    }
    if cache_action_step(
        true,
        TOOLS_RESTORE_ACTION_USES,
        "mbx-bundle",
        "bad key",
        &[MBX_BUNDLE_RESTORE_KEYS_EXPR.to_owned()],
        &[COMPLETE_STATE_TRANSPORT_EXPR.to_owned()],
    )
    .is_ok()
    {
        return Err(fail("state transport accepted its bad key"));
    }
    Ok(())
}

#[test]
fn gc1_t30_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_explicit_selection()?;
    check_rejects_bad_transport_inputs()?;
    Ok(())
}
