// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn reject_payload(
    setup: &MiseSetup,
    label: &str,
    runs_on: &str,
    target: &str,
    specs: &[&str],
    toolchain: Option<&str>,
    components: &[&str],
) -> Result<(), Box<dyn Error>> {
    if payload(setup, runs_on, target, specs, toolchain, components).is_ok() {
        return Err(fail(&format!("invalid host tool set accepted: {label}")));
    }
    Ok(())
}

pub(crate) fn check_rejects_bad_lanes_and_targets(setup: &MiseSetup) -> Result<(), Box<dyn Error>> {
    reject_payload(
        setup,
        "empty lane",
        "",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2"],
        None,
        &[],
    )?;
    reject_payload(
        setup,
        "spaced lane",
        "ubuntu 24.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2"],
        None,
        &[],
    )?;
    reject_payload(
        setup,
        "bad target",
        "ubuntu-24.04",
        "not-a-target",
        &["gh@2.103.2"],
        None,
        &[],
    )?;
    reject_payload(
        setup,
        "empty specs",
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        &[],
        None,
        &[],
    )?;
    Ok(())
}

pub(crate) fn check_rejects_bad_specs(setup: &MiseSetup) -> Result<(), Box<dyn Error>> {
    reject_payload(
        setup,
        "spec without version",
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        &["gh"],
        None,
        &[],
    )?;
    reject_payload(
        setup,
        "conflicting specs",
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2", "gh@2.103.3"],
        None,
        &[],
    )?;
    reject_payload(
        setup,
        "rust without toolchain",
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        &["rust@1.99.1"],
        None,
        &[],
    )?;
    reject_payload(
        setup,
        "toolchain without rust",
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2"],
        Some("1.99.1"),
        &[],
    )?;
    reject_payload(
        setup,
        "components without rust",
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2"],
        None,
        &["clippy"],
    )?;
    reject_payload(
        setup,
        "unknown component",
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        &["rust@1.99.1"],
        Some("1.99.1"),
        &["cargo"],
    )?;
    Ok(())
}

pub(crate) fn check_rejects_bad_setup_and_identity(
    setup: &MiseSetup,
) -> Result<(), Box<dyn Error>> {
    let mut bad_setup = mise_setup();
    "not-hex".clone_into(&mut bad_setup.sha256);
    reject_payload(
        &bad_setup,
        "bad mise sha",
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2"],
        None,
        &[],
    )?;
    let owned = base_payload(setup)?;
    if owned.key_for_runtime_identity("short").is_ok()
        || owned.key_for_runtime_identity(&"zz".repeat(32)).is_ok()
    {
        return Err(fail("invalid runtime identity accepted"));
    }
    let unsupported = payload(
        setup,
        "ubuntu-20.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2", "opentofu@1.13.2"],
        None,
        &[],
    )?;
    if unsupported.runtime_identity_supported() {
        return Err(fail("unqualified lane kept runtime identity"));
    }
    if unsupported.runtime_prelude_step().is_ok() {
        return Err(fail("unqualified lane emitted a prelude"));
    }
    Ok(())
}

pub(crate) fn check_rejects_bad_saves(setup: &MiseSetup) -> Result<(), Box<dyn Error>> {
    let owned = base_payload(setup)?;
    let mut elected: BTreeMap<String, Job> = BTreeMap::from([("a".to_owned(), job(&owned)?)]);
    elect_cache_writers(&mut elected)?;
    let save = elected
        .get("a")
        .and_then(|job| {
            job.steps
                .iter()
                .find(|step| step.role == Some(StepRole::ToolsCacheSave))
        })
        .cloned()
        .ok_or_else(|| fail("common save missing"))?;
    let mut orphan: BTreeMap<String, Job> = BTreeMap::new();
    orphan.insert(
        "c".to_owned(),
        Job {
            steps: vec![save.clone()],
            ..job(&owned)?
        },
    );
    if elect_cache_writers(&mut orphan).is_ok() {
        return Err(fail("orphan tools save accepted"));
    }
    let mut crowded: BTreeMap<String, Job> = BTreeMap::from([
        ("a".to_owned(), job(&owned)?),
        ("b".to_owned(), job(&owned)?),
    ]);
    crowded
        .get_mut("b")
        .ok_or_else(|| fail("crowded job missing"))?
        .steps
        .push(save.clone());
    if elect_cache_writers(&mut crowded).is_ok() {
        return Err(fail("unelected tools save accepted"));
    }
    let mut tampered: BTreeMap<String, Job> = BTreeMap::from([("a".to_owned(), job(&owned)?)]);
    elect_cache_writers(&mut tampered)?;
    let tampered_job = tampered
        .get_mut("a")
        .ok_or_else(|| fail("tampered job missing"))?;
    let index = save_steps(tampered_job)
        .into_iter()
        .next()
        .ok_or_else(|| fail("tampered save missing"))?;
    tampered_job.steps[index].condition = Some("always()".to_owned());
    if elect_cache_writers(&mut tampered).is_ok() {
        return Err(fail("independent tools save accepted"));
    }
    let mut doubled: BTreeMap<String, Job> = BTreeMap::from([("a".to_owned(), job(&owned)?)]);
    let extra = owned.restore_step()?;
    doubled
        .get_mut("a")
        .ok_or_else(|| fail("doubled job missing"))?
        .steps
        .push(extra);
    if elect_cache_writers(&mut doubled).is_ok() {
        return Err(fail("duplicate tools restore accepted"));
    }
    Ok(())
}

#[test]
fn gc1_t13_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let setup = mise_setup();
    check_rejects_bad_lanes_and_targets(&setup)?;
    check_rejects_bad_specs(&setup)?;
    check_rejects_bad_setup_and_identity(&setup)?;
    check_rejects_bad_saves(&setup)?;
    Ok(())
}
