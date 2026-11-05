// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_workflow_renderer::steps::MbxTransport;

use crate::gc1_cache_service::CacheService;
use crate::gc1_scene::{Key, Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh, MAIN, run_select, selected};

const SNAPSHOT_A: &str = "s256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SNAPSHOT_B: &str = "s256-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PAYLOAD_ONE: &str = "s256-1111111111111111111111111111111111111111111111111111111111111111";
const PAYLOAD_TWO: &str = "s256-2222222222222222222222222222222222222222222222222222222222222222";
const OUTPUT_OPEN: &str = "${{steps.mbx-bundle-key.outputs.";
const T1: &str = "2026-11-04T01:00:00Z";
const T2: &str = "2026-11-04T02:00:00Z";

fn restore(
    scene: &Scene,
    service: &CacheService,
    root: Option<&str>,
    attested: &[String],
    env: &[(&str, &str)],
) -> Result<Option<String>, Box<dyn Error>> {
    let list = attested.join("\n");
    let mut pairs = vec![("VELNOR_STATE_ATTESTED", list.as_str())];
    pairs.extend_from_slice(env);
    scene.discover_with(&pairs)?;
    let with = crate::gc1_t53::restore_with(MbxTransport::SeedRead, root)?;
    if with.contains_key("restore-keys") {
        return Err(fail("restore step kept a prefix fallback"));
    }
    let output = with
        .get("key")
        .and_then(|key| key.strip_prefix(OUTPUT_OPEN))
        .and_then(|key| key.strip_suffix("}}"))
        .ok_or_else(|| fail("restore key is not a discover output"))?;
    Ok(service.restore(&scene.output_value(output)?, &[]))
}

fn expect(got: Option<&String>, want: Option<&str>, label: &str) -> Result<(), Box<dyn Error>> {
    if got.map(String::as_str) == want {
        Ok(())
    } else {
        Err(fail(&format!("{label}: picked {got:?}, wanted {want:?}")))
    }
}

fn plan(
    scene: &Scene,
    gh: &Gh,
    root: &str,
    rows: &[(&str, &str, &str)],
    attested: &[&str],
) -> Result<Vec<String>, Box<dyn Error>> {
    gh.list(rows)?;
    gh.attest(
        &attested
            .iter()
            .map(|key| Attestation::signed(key))
            .collect::<Vec<_>>(),
    )?;
    run_select(scene, gh, root, &[])?;
    selected(scene, root)
}

#[test]
#[cfg(unix)]
fn restore_takes_only_the_key_plan_selected() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("v4-restore", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let other = scene.discover_with(&[("ImageOS", "ubuntu24")])?;
    let id = scene.discover()?;
    let sealed = format!("{}{PAYLOAD_ONE}", id.snapshot_prefix);
    let forged = format!("{}{PAYLOAD_TWO}", id.snapshot_prefix);
    let predecessor = format!("{}{SNAPSHOT_A}-{PAYLOAD_ONE}", id.restore_prefix);
    let foreign = format!("{}{SNAPSHOT_B}-{PAYLOAD_ONE}", other.restore_prefix);

    let mut service = CacheService::new();
    service.save(&sealed, 10);
    service.save(&predecessor, 5);
    service.save(&forged, 30);
    service.save(&foreign, 40);

    let both = plan(
        &scene,
        &gh,
        ".",
        &[(T2, MAIN, &forged), (T1, MAIN, &sealed)],
        &[&sealed],
    )?;
    expect(
        restore(&scene, &service, None, &both, &[])?.as_ref(),
        Some(&sealed),
        "attested exact entry beats a newer forged one under the same prefix",
    )?;
    if scene.output_value("exact")? != "true" {
        return Err(fail(
            "an attested entry at the current snapshot is not exact",
        ));
    }

    let older = plan(
        &scene,
        &gh,
        ".",
        &[(T2, MAIN, &forged), (T1, MAIN, &predecessor)],
        &[&predecessor],
    )?;
    expect(
        restore(&scene, &service, None, &older, &[])?.as_ref(),
        Some(&predecessor),
        "attested predecessor of another snapshot",
    )?;
    if scene.output_value("exact")? != "false" {
        return Err(fail("a predecessor entry reported exact"));
    }

    let wrong_compat = plan(&scene, &gh, ".", &[(T2, MAIN, &foreign)], &[&foreign])?;
    expect(
        restore(&scene, &service, None, &wrong_compat, &[])?.as_ref(),
        None,
        "attested entry of another compat",
    )?;

    gh.mode("fail")?;
    let none = plan(
        &scene,
        &gh,
        ".",
        &[(T2, MAIN, &forged), (T1, MAIN, &sealed)],
        &[&sealed],
    )?;
    expect(
        restore(&scene, &service, None, &none, &[])?.as_ref(),
        None,
        "empty selection with forged and attested entries in the service",
    )?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn restore_never_crosses_roots_that_share_a_name_prefix() -> Result<(), Box<dyn Error>> {
    let short = Scene::new(
        "v4-root-a",
        crate::gc1_t53::scripts_for(MbxTransport::SeedRead, Some("a"))?,
    )?;
    let gh = Gh::install(&short)?;
    let short_key: Key = short.discover_with(&[("VELNOR_MBX_BUNDLE_ROOT", "a")])?;
    let long_entry = format!(
        "{}{SNAPSHOT_A}-{PAYLOAD_ONE}",
        short_key
            .restore_prefix
            .replacen("-slice-a-", "-slice-a-b-", 1)
    );
    let own_entry = format!("{}{SNAPSHOT_A}-{PAYLOAD_TWO}", short_key.restore_prefix);
    let mut service = CacheService::new();
    service.save(&long_entry, 50);
    service.save(&own_entry, 1);

    let crossed = plan(&short, &gh, "a", &[(T2, MAIN, &long_entry)], &[&long_entry])?;
    expect(
        restore(
            &short,
            &service,
            Some("a"),
            &crossed,
            &[("VELNOR_MBX_BUNDLE_ROOT", "a")],
        )?
        .as_ref(),
        None,
        "root a restored an entry of root a-b",
    )?;
    let own = plan(
        &short,
        &gh,
        "a",
        &[(T2, MAIN, &long_entry), (T1, MAIN, &own_entry)],
        &[&long_entry, &own_entry],
    )?;
    expect(
        restore(
            &short,
            &service,
            Some("a"),
            &own,
            &[("VELNOR_MBX_BUNDLE_ROOT", "a")],
        )?
        .as_ref(),
        Some(&own_entry),
        "root a keeps its own entry",
    )?;
    short.finish()
}
