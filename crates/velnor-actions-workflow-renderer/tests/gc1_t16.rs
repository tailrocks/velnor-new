// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use velnor_actions_contract::{StepId, StepKind, StepRole};
use velnor_actions_workflow_renderer::cache_p08::{ToolsCacheInputs, ToolsCachePayload};
use velnor_actions_workflow_renderer::setup::{MISE_BINARY_SHA256_LINUX_X64, MiseSetup};
use velnor_actions_workflow_renderer::toolchain_env::{
    SHARED_CARGO_HOME, SHARED_OBSERVATION_ROOT, SHARED_OWNED_ROOT, SHARED_RUSTUP_HOME,
    SHARED_SOURCE_ROOT, SHARED_TOOL_ROOT, SHARED_TRANSPORT_ROOT, SHARED_WORKSPACE_ROOT,
    SharedRunnerHomes, check_toolchain_homes, checked_project_task_env, checked_task_env,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn payload() -> Result<ToolsCachePayload, Box<dyn Error>> {
    let setup = MiseSetup {
        uses: format!(
            "jdx/mise-action@{}",
            velnor_actions_actionlint::actions::MISE_ACTION_SHA
        ),
        version: velnor_actions_mise::MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    };
    let specs = ["gh@2.103.2".to_owned(), "opentofu@1.13.2".to_owned()];
    Ok(ToolsCachePayload::new(ToolsCacheInputs {
        runs_on: "ubuntu-24.04",
        target: "x86_64-unknown-linux-gnu",
        mise_setup: &setup,
        tool_specs: &specs,
        rustup_toolchain: None,
        rustup_components: &[],
    })?)
}

fn check_canonical_roots() -> Result<(), Box<dyn Error>> {
    let first = SharedRunnerHomes::github();
    let second = SharedRunnerHomes::github();
    if first != second {
        return Err(fail("shared runner homes drifted between builds"));
    }
    first.validate()?;
    if first.workspace_root != SHARED_WORKSPACE_ROOT || first.workspace_root != "$GITHUB_WORKSPACE"
    {
        return Err(fail("execution directory left the actual checkout"));
    }
    if first.owned_root != SHARED_OWNED_ROOT {
        return Err(fail("owned root left its deterministic runner path"));
    }
    for (field, value) in [
        ("source_root", first.source_root.as_str()),
        ("tool_root", first.tool_root.as_str()),
        ("cargo_home", first.cargo_home.as_str()),
        ("rustup_home", first.rustup_home.as_str()),
        ("transport_root", first.transport_root.as_str()),
        ("observation_root", first.observation_root.as_str()),
    ] {
        if !value.starts_with(&format!("{SHARED_OWNED_ROOT}/")) {
            return Err(fail(&format!("shared root outside owned root: {field}")));
        }
    }
    if first.source_root != SHARED_SOURCE_ROOT
        || first.tool_root != SHARED_TOOL_ROOT
        || first.cargo_home != SHARED_CARGO_HOME
        || first.rustup_home != SHARED_RUSTUP_HOME
        || first.transport_root != SHARED_TRANSPORT_ROOT
        || first.observation_root != SHARED_OBSERVATION_ROOT
    {
        return Err(fail("shared root missed its deterministic runner path"));
    }
    if first.rustup_home == first.cargo_home {
        return Err(fail("shared toolchain homes collapsed"));
    }
    Ok(())
}

fn check_checked_env_applies_shared_homes() -> Result<(), Box<dyn Error>> {
    let homes = SharedRunnerHomes::github();
    let env = homes.checked_env(&BTreeMap::new(), "1.99.1")?;
    for (key, expected) in [
        ("MISE_RUSTUP_HOME", SHARED_RUSTUP_HOME),
        ("RUSTUP_HOME", SHARED_RUSTUP_HOME),
        ("MISE_CARGO_HOME", SHARED_CARGO_HOME),
        ("CARGO_HOME", SHARED_CARGO_HOME),
        ("RUSTUP_TOOLCHAIN", "1.99.1"),
    ] {
        if env.get(key).map(String::as_str) != Some(expected) {
            return Err(fail(&format!("shared env missed its owned path: {key}")));
        }
    }
    check_toolchain_homes(&env)?;
    let legacy = BTreeMap::from([
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        ("MISE_CARGO_HOME".to_owned(), "$task_root/cargo".to_owned()),
        ("CARGO_HOME".to_owned(), "$task_root/cargo".to_owned()),
    ]);
    let reused = homes.checked_env(&legacy, "")?;
    for (key, expected) in [
        ("MISE_RUSTUP_HOME", SHARED_RUSTUP_HOME),
        ("RUSTUP_HOME", SHARED_RUSTUP_HOME),
        ("MISE_CARGO_HOME", SHARED_CARGO_HOME),
        ("CARGO_HOME", SHARED_CARGO_HOME),
    ] {
        if reused.get(key).map(String::as_str) != Some(expected) {
            return Err(fail(&format!("disposable home assignment survived: {key}")));
        }
    }
    if reused.contains_key("RUSTUP_TOOLCHAIN") {
        return Err(fail("empty toolchain leaked into shared env"));
    }
    Ok(())
}

fn check_wrapper_inherits_shared_context() -> Result<(), Box<dyn Error>> {
    let owned = payload()?;
    let prelude = owned.runtime_prelude_step()?;
    if prelude.role != Some(StepRole::ToolsCacheIdentity)
        || prelude.id != Some(StepId::ToolsCacheIdentity)
    {
        return Err(fail("prelude step lost its tools identity"));
    }
    let StepKind::Action { env, with, .. } = &prelude.kind else {
        return Err(fail("prelude step is not an action step"));
    };
    if !env.is_empty() {
        return Err(fail("prelude wrapper assigned its own homes"));
    }
    if with.len() != 1 {
        return Err(fail("prelude step left its single digest input"));
    }
    let restore = owned.restore_step()?;
    if restore.role != Some(StepRole::ToolsCacheRestore) || restore.id != Some(StepId::ToolsRestore)
    {
        return Err(fail("restore step lost its tools identity"));
    }
    let env = checked_task_env(
        &BTreeMap::new(),
        "${{ runner.temp }}/velnor/rustup",
        "${{ runner.temp }}/velnor/cargo",
        "1.99.1",
    )?;
    check_toolchain_homes(&env)?;
    checked_project_task_env(
        &BTreeMap::new(),
        "${{ runner.temp }}/velnor/rustup",
        "${{ runner.temp }}/velnor/cargo",
        "1.99.1",
    )?;
    Ok(())
}

#[test]
fn gc1_t16_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_canonical_roots()?;
    check_checked_env_applies_shared_homes()?;
    check_wrapper_inherits_shared_context()?;
    Ok(())
}

fn check_rejects_bad_roots() -> Result<(), Box<dyn Error>> {
    let bad = SharedRunnerHomes {
        workspace_root: "${{ github.workspace }}".to_owned(),
        ..SharedRunnerHomes::github()
    };
    if bad.validate().is_ok() {
        return Err(fail("workspace drift accepted"));
    }
    let bad = SharedRunnerHomes {
        owned_root: "$GITHUB_WORKSPACE/velnor-gc1".to_owned(),
        ..SharedRunnerHomes::github()
    };
    if bad.validate().is_ok() {
        return Err(fail("owned root inside the checkout accepted"));
    }
    let bad = SharedRunnerHomes {
        tool_root: "/tmp/velnor-gc1/tools".to_owned(),
        ..SharedRunnerHomes::github()
    };
    if bad.validate().is_ok() {
        return Err(fail("tool root outside the owned root accepted"));
    }
    let canonical = SharedRunnerHomes::github();
    let bad = SharedRunnerHomes {
        observation_root: canonical.transport_root.clone(),
        ..canonical
    };
    if bad.validate().is_ok() {
        return Err(fail("overlapping shared roots accepted"));
    }
    let bad = SharedRunnerHomes {
        source_root: String::new(),
        ..SharedRunnerHomes::github()
    };
    if bad.validate().is_ok() {
        return Err(fail("empty shared root accepted"));
    }
    let bad = SharedRunnerHomes {
        transport_root: format!("{SHARED_OWNED_ROOT}/../escape"),
        ..SharedRunnerHomes::github()
    };
    if bad.validate().is_ok() {
        return Err(fail("escaping shared root accepted"));
    }
    for (field, value) in [
        ("control", "cargo\nHOME=x"),
        ("substitution", "$(id)"),
        ("backtick", "`id`"),
    ] {
        let bad = SharedRunnerHomes {
            cargo_home: format!("{SHARED_OWNED_ROOT}/{value}"),
            ..SharedRunnerHomes::github()
        };
        if bad.checked_env(&BTreeMap::new(), "1.99.1").is_ok() {
            return Err(fail(&format!("tainted shared root accepted: {field}")));
        }
    }
    Ok(())
}

fn check_rejects_conflicting_assignments() -> Result<(), Box<dyn Error>> {
    let homes = SharedRunnerHomes::github();
    let disposable = BTreeMap::from([("HOME".to_owned(), "$task_root/home".to_owned())]);
    if homes.checked_env(&disposable, "1.99.1").is_ok() {
        return Err(fail("disposable HOME accepted into shared env"));
    }
    if checked_task_env(&disposable, SHARED_RUSTUP_HOME, SHARED_CARGO_HOME, "1.99.1").is_ok() {
        return Err(fail("disposable HOME accepted into task env"));
    }
    if checked_project_task_env(&disposable, SHARED_RUSTUP_HOME, SHARED_CARGO_HOME, "1.99.1")
        .is_ok()
    {
        return Err(fail("disposable HOME accepted into project task env"));
    }
    let credential = BTreeMap::from([("GITHUB_TOKEN".to_owned(), "x".to_owned())]);
    if homes.checked_env(&credential, "1.99.1").is_ok() {
        return Err(fail("credential step env accepted into shared env"));
    }
    let privileged = BTreeMap::from([("MISE_NO_CONFIG".to_owned(), "1".to_owned())]);
    if homes.checked_env(&privileged, "1.99.1").is_ok() {
        return Err(fail("privileged task env accepted into shared env"));
    }
    let mut broken = SharedRunnerHomes::github();
    broken.cargo_home = String::new();
    if broken.checked_env(&BTreeMap::new(), "1.99.1").is_ok() {
        return Err(fail("empty shared cargo home accepted"));
    }
    Ok(())
}

#[test]
fn gc1_t16_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_roots()?;
    check_rejects_conflicting_assignments()?;
    Ok(())
}
