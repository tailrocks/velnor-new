// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{
    Attestation, Gh, MAIN, PULL, REPO, digest, key, run_select, scope, selected,
};

const ROOT: &str = ".";
const T1: &str = "2026-11-04T01:00:00Z";
const T2: &str = "2026-11-04T02:00:00Z";
const T3: &str = "2026-11-04T03:00:00Z";

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
