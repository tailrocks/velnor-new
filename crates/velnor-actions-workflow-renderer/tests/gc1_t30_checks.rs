// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_discovery_binds_provenance(key: &Step) -> Result<(), Box<dyn Error>> {
    if key.name != MBX_BUNDLE_KEY_NAME
        || key.id != Some(velnor_actions_contract::StepId::MbxBundleKey)
        || key.role != Some(StepRole::MbxBundleKey)
    {
        return Err(fail("discovery step lost its state identity"));
    }
    let script = shell_script(key)?;
    for marker in [
        "'family=state'",
        "'schema=1'",
        "$RUNNER_TEMP/velnor-gc1/state",
        "\"key=$key\"",
        "\"restore_keys=$trusted\"",
        "\"save=$save\"",
        "\"provenance=$provenance\"",
        "\"decision=$decision\"",
        "state-discovery.record",
        "restore_prefix=",
        "compat=$compat",
    ] {
        if !script.contains(marker) {
            return Err(fail(&format!(
                "discovery step left the common snapshot discipline: {marker}"
            )));
        }
    }
    Ok(())
}

fn check_restore_uses_fixed_transport(restore: &Step) -> Result<(), Box<dyn Error>> {
    if restore.name != MBX_BUNDLE_RESTORE_NAME
        || restore.id != Some(velnor_actions_contract::StepId::MbxBundleRestore)
        || restore.role != Some(StepRole::MbxBundleRestore)
        || restore.condition.is_some()
    {
        return Err(fail("restore step lost its state identity"));
    }
    let (uses, with) = match &restore.kind {
        StepKind::Action { uses, with, .. } => (uses.as_str(), with),
        _ => return Err(fail("restore step left its pinned action")),
    };
    if uses != TOOLS_RESTORE_ACTION_USES {
        return Err(fail("restore step left its pinned restore action"));
    }
    if with.get("key").map(String::as_str) != Some(MBX_BUNDLE_KEY_EXPR) {
        return Err(fail("restore step left its selected key"));
    }
    if with.get("restore-keys").map(String::as_str) != Some(MBX_BUNDLE_RESTORE_KEYS_EXPR) {
        return Err(fail("restore step left its selected restore keys"));
    }
    let path = with
        .get("path")
        .ok_or_else(|| fail("restore step lost its path"))?;
    if path != COMPLETE_STATE_TRANSPORT_EXPR {
        return Err(fail("restore step left its fixed transport path"));
    }
    if path.contains("GITHUB_WORKSPACE") || path.contains('\n') {
        return Err(fail("restore path interpolated a per-job directory"));
    }
    Ok(())
}

fn check_recovery_is_verified_operation(import: &Step) -> Result<(), Box<dyn Error>> {
    if import.name != MBX_BUNDLE_IMPORT_NAME
        || import.id.is_some()
        || import.role != Some(StepRole::MbxBundleImport)
        || import.condition.as_deref() != Some(MBX_BUNDLE_IMPORT_CONDITION)
    {
        return Err(fail("recovery step lost its state identity"));
    }
    let script = shell_script(import)?;
    for marker in [
        "$RUNNER_TEMP/velnor-gc1/transport/state/payload.tar.gz",
        "$RUNNER_TEMP/velnor-gc1/state",
        "tar -tzf",
        "tar -xzf",
        "state-discovery.record",
        "refused-provenance",
        "refused-shadow",
        "exact-hit",
        "predecessor-fallback",
        "cold-missing",
        "coverage drift",
        "over ceiling",
        "readback mismatch",
        "state snapshot recovered (decision=",
        "state-recovery.record",
    ] {
        if !script.contains(marker) {
            return Err(fail(&format!(
                "recovery step left its verified operation: {marker}"
            )));
        }
    }
    for forbidden in [
        "mbx cache import",
        "$mbx_bin",
        "mr-boxington",
        "tar -xzf \"$payload\" -C \"$parent\"",
    ] {
        if script.contains(forbidden) {
            return Err(fail(&format!(
                "recovery step kept its raw tar route: {forbidden}"
            )));
        }
    }
    let screen = script
        .find("tar -tzf")
        .ok_or_else(|| fail("recovery lost its scan"))?;
    let extract = script
        .find("tar -xzf")
        .ok_or_else(|| fail("recovery lost its extract"))?;
    if screen > extract {
        return Err(fail("recovery extracts before screening members"));
    }
    Ok(())
}

fn check_recovery_refuses_shadow_family(import: &Step) -> Result<(), Box<dyn Error>> {
    let script = shell_script(import)?;
    for marker in [
        "\"$base\"|\"$base\"/*",
        "*/../*",
        "refused refused-shadow",
        "-type l >",
        "state-stage",
    ] {
        if !script.contains(marker) {
            return Err(fail(&format!(
                "recovery step lost its shadow-family refusal: {marker}"
            )));
        }
    }
    Ok(())
}

fn check_producer_mark(mark: &Step) -> Result<(), Box<dyn Error>> {
    if mark.role != Some(StepRole::MbxProducerMark) {
        return Err(fail("producer mark lost its role"));
    }
    let script = shell_script(mark)?;
    if !script.contains("$RUNNER_TEMP/velnor/state-producer-ok") {
        return Err(fail("producer mark left its fixed state marker"));
    }
    if script.contains("mbx-producer-ok") {
        return Err(fail("producer mark kept its bundle marker"));
    }
    Ok(())
}

fn check_save_shape() -> Result<(), Box<dyn Error>> {
    let mut save = cache_action_step(
        false,
        TOOLS_SAVE_USES,
        "mbx-bundle",
        MBX_BUNDLE_KEY_EXPR,
        &[],
        &[COMPLETE_STATE_TRANSPORT_EXPR.to_owned()],
    )?;
    if !matches!(&save.kind, StepKind::Action { uses, .. } if uses == TOOLS_SAVE_USES) {
        return Err(fail("save step left its pinned save action"));
    }
    let with = action_with(&save)?;
    if with.get("key").map(String::as_str) != Some(MBX_BUNDLE_KEY_EXPR) {
        return Err(fail("save step left its selected key"));
    }
    if with.contains_key("restore-keys") {
        return Err(fail("save step carried restore keys"));
    }
    if with.get("path").map(String::as_str) != Some(COMPLETE_STATE_TRANSPORT_EXPR) {
        return Err(fail("save step left its fixed transport path"));
    }
    save.condition = Some(MBX_BUNDLE_SAVE_CONDITION.to_owned());
    if save.condition.as_deref() != Some(MBX_BUNDLE_SAVE_CONDITION) {
        return Err(fail("save step left its typed admission gate"));
    }
    if !MBX_BUNDLE_SAVE_CONDITION.contains("steps.mbx-bundle-export.outputs.exported") {
        return Err(fail("save gate left its typed publish output"));
    }
    if !MBX_BUNDLE_EXPORT_CONDITION.contains("steps.mbx-bundle-key.outputs.save") {
        return Err(fail("publish gate left its typed policy output"));
    }
    Ok(())
}

fn check_inventory_install_accepted() -> Result<(), Box<dyn Error>> {
    for with in [
        BTreeMap::from([("install".to_owned(), "true".to_owned())]),
        BTreeMap::new(),
        BTreeMap::from([
            ("install".to_owned(), "false".to_owned()),
            ("tool_versions".to_owned(), "gh@2.103.2".to_owned()),
        ]),
    ] {
        let jobs = BTreeMap::from([("worker".to_owned(), job_with(vec![setup_step(with)]))]);
        if check_mbx_gating(&jobs, &BTreeMap::new()).is_err() {
            return Err(fail("inventory filtering rejected as action ref selection"));
        }
    }
    Ok(())
}

#[test]
fn gc1_t30_preserves_contract() -> Result<(), Box<dyn Error>> {
    let steps = transport_steps()?;
    let again = transport_steps()?;
    if steps != again {
        return Err(fail("state transport drifted between renders"));
    }
    let roles: Vec<Option<StepRole>> = steps.iter().map(|step| step.role).collect();
    if roles
        != [
            Some(StepRole::MbxPreflight),
            Some(StepRole::MbxCache),
            Some(StepRole::MbxVersionCheck),
            Some(StepRole::MbxBundleKey),
            Some(StepRole::MbxBundleRestore),
            Some(StepRole::MbxBundleImport),
        ]
    {
        return Err(fail("bundle prelude lost its transport lifecycle"));
    }
    check_discovery_binds_provenance(&steps[3])?;
    check_shell_syntax(&steps[3], "discovery")?;
    check_restore_uses_fixed_transport(&steps[4])?;
    check_recovery_is_verified_operation(&steps[5])?;
    check_recovery_refuses_shadow_family(&steps[5])?;
    check_shell_syntax(&steps[5], "recovery")?;
    check_producer_mark(&mbx_producer_mark_step()?)?;
    check_save_shape()?;
    check_inventory_install_accepted()?;
    Ok(())
}
