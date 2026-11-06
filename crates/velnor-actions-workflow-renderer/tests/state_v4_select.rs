// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::workflow::StepKind;
use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, RootSlug, StateUnit, outer_entry_bound,
};
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_LIST_LIMIT, STATE_SEAL_VERIFY_LIMIT, state_attested_output, state_produce_output,
};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{
    Attestation, ENTRY_ID, ENTRY_SIZE, Gh, MAIN, PULL, REPO, digest, key, run_select, scope,
    selected,
};
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealRoot, state_select_step};

const ROOT: &str = ".";
const T1: &str = "2026-11-04T01:00:00Z";
const T2: &str = "2026-11-04T02:00:00Z";
const T3: &str = "2026-11-04T03:00:00Z";

#[test]
fn selector_keeps_supervisor_in_static_environment_and_loader_in_run() -> Result<(), Box<dyn Error>>
{
    let step = state_select_step(
        &[SealRoot {
            unit: StateUnit::Union,
            slug: RootSlug::from_root(ROOT)?,
        }],
        JobTimeout::PLAN,
    )?;
    let StepKind::Shell { run, env } = step.kind else {
        return Err(fail("selector step is not a shell step"));
    };
    let expected_record = format!(
        "{} {} {}",
        scope(StateUnit::Union, ROOT)?.prefix(),
        state_attested_output("union", "root"),
        state_produce_output("union", "root")
    );
    if env
        .get("VELNOR_STATE_SELECT_UNIT_COUNT")
        .map(String::as_str)
        != Some("1")
        || env.get("VELNOR_STATE_SELECT_UNIT_R0").map(String::as_str)
            != Some(expected_record.as_str())
    {
        return Err(fail("selector typed unit manifest differs from its root"));
    }
    let payload = env
        .get("VELNOR_STATE_SELECT_SUPERVISOR_HEX")
        .ok_or_else(|| fail("selector supervisor environment is absent"))?;
    if payload.len() > 49_152
        || payload.is_empty()
        || payload.len() % 2 != 0
        || !payload
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(fail(
            "selector supervisor environment is not bounded lowercase hex",
        ));
    }
    let decoded = payload
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or_else(|| fail("bad supervisor hex"))?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or_else(|| fail("bad supervisor hex"))?;
            Ok((high * 16 + low) as u8)
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let source = String::from_utf8(decoded)?;
    let supervisor_file = include_str!("../src/plan_sources/state_select_supervisor.rs");
    let literal = supervisor_file
        .split_once("r#\"")
        .and_then(|(_, tail)| tail.split_once("\"#"))
        .map(|(source, _)| source)
        .ok_or_else(|| fail("supervisor source literal is missing"))?;
    let expected = literal.replace("@MAX_DEADLINE@", "3600");
    if source != expected {
        return Err(fail(
            "selector payload differs from the substituted supervisor source",
        ));
    }
    let script = run.last().ok_or_else(|| fail("selector run is empty"))?;
    if script.contains(payload)
        || !script.contains("python3 -I -c 'import os; exec(bytes.fromhex(os.environ.pop")
    {
        return Err(fail(
            "selector run does not load and remove its static payload",
        ));
    }
    Ok(())
}

fn expect(got: &[String], want: &[&str], label: &str) -> Result<(), Box<dyn Error>> {
    if got == want {
        Ok(())
    } else {
        Err(fail(&format!("{label}: selected {got:?}, wanted {want:?}")))
    }
}

fn forgeries(newest: &str) -> Vec<(&'static str, Attestation)> {
    let signed = || Attestation::signed(newest);
    vec![
        (
            "pull-request ref",
            Attestation {
                git_ref: PULL.to_owned(),
                ..signed()
            },
        ),
        (
            "other signer workflow",
            Attestation {
                signer: format!("{REPO}/.github/workflows/release.yml"),
                ..signed()
            },
        ),
        (
            "previous signer workflow ci.yml",
            Attestation {
                signer: format!("{REPO}/.github/workflows/ci.yml"),
                ..signed()
            },
        ),
        (
            "other predicate type",
            Attestation {
                predicate: "https://slsa.dev/provenance/v1".to_owned(),
                ..signed()
            },
        ),
        (
            "other repository",
            Attestation {
                repo: "attacker/velnor-new".to_owned(),
                ..signed()
            },
        ),
        (
            "subject of another key",
            Attestation::signed(&format!("{newest}-other")),
        ),
    ]
}

#[test]
#[cfg(unix)]
fn plan_selects_only_the_newest_attested_entry() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let newest = key('a', '1', '1')?;
    let older = key('a', '1', '2')?;
    let oldest = key('a', '2', '3')?;

    gh.list(&[(T3, MAIN, &newest), (T2, MAIN, &older), (T1, MAIN, &oldest)])?;
    gh.attest(&[
        Attestation::signed(&older),
        Attestation::signed(&newest),
        Attestation::signed(&oldest),
    ])?;
    run_select(&scene, &gh, ROOT, &[])?;
    expect(
        &selected(&scene, ROOT)?,
        &[&newest, &older, &oldest],
        "attested entries, newest first",
    )?;

    gh.list(&[(T3, MAIN, &newest), (T2, MAIN, &older)])?;
    gh.attest(&[Attestation::signed(&older)])?;
    run_select(&scene, &gh, ROOT, &[])?;
    expect(
        &selected(&scene, ROOT)?,
        &[&older],
        "unattested newer entry",
    )?;

    for (label, forged) in forgeries(&newest) {
        gh.attest(&[Attestation::signed(&older), forged])?;
        run_select(&scene, &gh, ROOT, &[])?;
        expect(&selected(&scene, ROOT)?, &[&older], label)?;
    }

    let prefix = scope()?.prefix();
    let foreign = [
        prefix.replacen("-union-", "-suite-", 1),
        prefix.replacen("-trusted-", "-pr-", 1),
        prefix.replacen("-root-", "-other-", 1),
        format!("{prefix}s256-nothex"),
        format!("{prefix}{}-{}", digest('a'), digest('b')),
    ];
    let mut rows = vec![(T3, MAIN, older.as_str())];
    let stray: Vec<String> = foreign
        .iter()
        .map(|stem| format!("{stem}{}-{}-{}", digest('a'), digest('1'), digest('9')))
        .collect();
    rows.extend(stray.iter().map(|entry| (T3, MAIN, entry.as_str())));
    rows.push((T3, PULL, newest.as_str()));
    gh.list(&rows)?;
    let mut attested = vec![Attestation::signed(&older), Attestation::signed(&newest)];
    attested.extend(stray.iter().map(|entry| Attestation::signed(entry)));
    gh.attest(&attested)?;
    run_select(&scene, &gh, ROOT, &[])?;
    expect(
        &selected(&scene, ROOT)?,
        &[&older],
        "foreign and unparsable entries",
    )?;

    gh.list(&[(T3, MAIN, &newest), (T2, MAIN, &older)])?;
    gh.attest(&[Attestation::signed(&newest), Attestation::signed(&older)])?;
    let outages: &[(&str, &[(&str, &str)])] = &[
        ("fail", &[]),
        ("garbage", &[]),
        #[cfg(target_os = "linux")]
        ("hang", &[("VELNOR_STATE_GH_TIMEOUT", "1")]),
    ];
    for (mode, extra) in outages {
        gh.mode(mode)?;
        run_select(&scene, &gh, ROOT, extra)?;
        expect(
            &selected(&scene, ROOT)?,
            &[],
            &format!("verifier outage {mode}"),
        )?;
    }
    gh.mode("ok")?;
    run_select(&scene, &gh, ROOT, &[])?;
    expect(
        &selected(&scene, ROOT)?,
        &[&newest, &older],
        "outage cleared",
    )?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn plan_selects_only_the_entry_generation_that_was_attested() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-generation", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let sealed = key('a', '1', '1')?;
    gh.attest(&[Attestation::bound(&sealed, ENTRY_ID, ENTRY_SIZE)])?;

    gh.list_entries(&[(T1, MAIN, &sealed, ENTRY_ID + 1, ENTRY_SIZE)])?;
    run_select(&scene, &gh, ROOT, &[])?;
    expect(
        &selected(&scene, ROOT)?,
        &[],
        "same key resaved under a new cache id",
    )?;

    gh.list_entries(&[(T1, MAIN, &sealed, ENTRY_ID, ENTRY_SIZE + 1)])?;
    run_select(&scene, &gh, ROOT, &[])?;
    expect(
        &selected(&scene, ROOT)?,
        &[],
        "same key and id with other bytes",
    )?;

    gh.list_entries(&[
        (T2, MAIN, &sealed, ENTRY_ID + 1, ENTRY_SIZE),
        (T1, MAIN, &sealed, ENTRY_ID, ENTRY_SIZE),
    ])?;
    run_select(&scene, &gh, ROOT, &[])?;
    expect(
        &selected(&scene, ROOT)?,
        &[&sealed],
        "the attested generation beside a newer resaved one",
    )?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn plan_selects_nothing_when_only_the_verifier_fails() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-verifier", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let sealed = key('a', '1', '1')?;
    gh.list(&[(T1, MAIN, &sealed)])?;
    gh.attest(&[Attestation::signed(&sealed)])?;

    let outages: &[(&str, &[(&str, &str)])] = &[
        ("verify-fail", &[]),
        ("verify-garbage", &[]),
        #[cfg(target_os = "linux")]
        ("verify-hang", &[("VELNOR_STATE_GH_TIMEOUT", "1")]),
    ];
    for (mode, extra) in outages {
        gh.mode(mode)?;
        run_select(&scene, &gh, ROOT, extra)?;
        expect(
            &selected(&scene, ROOT)?,
            &[],
            &format!("verifier outage {mode}"),
        )?;
    }
    gh.mode("ok")?;
    run_select(&scene, &gh, ROOT, &[])?;
    expect(&selected(&scene, ROOT)?, &[&sealed], "verifier restored")?;
    scene.finish()
}

fn numbered_key(ordinal: usize) -> Result<String, Box<dyn Error>> {
    Ok(format!(
        "{}s256-{:064x}-{}-{}",
        scope()?.prefix(),
        ordinal,
        digest('1'),
        digest('9')
    ))
}

#[test]
#[cfg(unix)]
fn plan_counts_only_verified_entries_against_its_limit() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-unattested-run", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let unattested: Vec<String> = (0..STATE_SEAL_VERIFY_LIMIT)
        .map(numbered_key)
        .collect::<Result<_, _>>()?;
    let attested = numbered_key(STATE_SEAL_VERIFY_LIMIT)?;

    let times: Vec<String> = (0..STATE_SEAL_VERIFY_LIMIT)
        .map(|offset| format!("2026-11-04T{:02}:00:00Z", 4 + offset))
        .collect();
    let mut rows: Vec<(&str, &str, &str)> = unattested
        .iter()
        .zip(&times)
        .map(|(entry, at)| (at.as_str(), MAIN, entry.as_str()))
        .collect();
    rows.push((T1, MAIN, attested.as_str()));
    gh.list(&rows)?;
    gh.attest(&[Attestation::signed(&attested)])?;
    run_select(&scene, &gh, ROOT, &[])?;
    expect(
        &selected(&scene, ROOT)?,
        &[&attested],
        "older attested entry behind newer unattested ones",
    )?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn plan_lists_by_creation_not_by_last_access() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-creation-order", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let fresh = numbered_key(0)?;
    let stale: Vec<String> = (1..=STATE_SEAL_LIST_LIMIT)
        .map(numbered_key)
        .collect::<Result<_, _>>()?;

    let mut rows: Vec<(&str, &str, &str, &str)> = vec![(T3, MAIN, fresh.as_str(), T1)];
    rows.extend(stale.iter().map(|entry| (T1, MAIN, entry.as_str(), T3)));
    gh.list_accessed(&rows)?;
    gh.attest(&[Attestation::signed(&fresh)])?;
    run_select(&scene, &gh, ROOT, &[])?;
    expect(
        &selected(&scene, ROOT)?,
        &[&fresh],
        "newest creation with the oldest access",
    )?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn plan_skips_entries_above_the_outer_bound() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-outer-bound", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let bound = outer_entry_bound(CacheFamily::MbxBundle.spec().ceiling.stored())
        .ok_or_else(|| fail("the outer bound is unbounded"))?;
    let over = key('a', '1', '1')?;
    let at = key('a', '1', '2')?;
    gh.list_entries(&[
        (T2, MAIN, &over, ENTRY_ID, bound + 1),
        (T1, MAIN, &at, ENTRY_ID + 1, bound),
    ])?;
    gh.attest(&[
        Attestation::bound(&over, ENTRY_ID, bound + 1),
        Attestation::bound(&at, ENTRY_ID + 1, bound),
    ])?;
    run_select(&scene, &gh, ROOT, &[])?;
    expect(
        &selected(&scene, ROOT)?,
        &[&at],
        "an attested entry above the outer bound",
    )?;
    scene.finish()
}
