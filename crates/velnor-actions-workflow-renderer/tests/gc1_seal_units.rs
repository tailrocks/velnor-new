// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::PathBuf;

use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::workflow::cache_family::{
    RootSlug, StateScope, StateTrust, StateUnit,
};
use velnor_actions_contract::workflow::state_seal::{state_attested_output, state_produce_output};
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealRoot, state_select_step};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Gh, MAIN, OTHER_WORKFLOW_REF, REPO, script_of};

#[cfg(target_os = "linux")]
pub(crate) fn hang_keys(gh: &Gh, keys: &[&str]) -> Result<(), Box<dyn Error>> {
    let text = format!("{}\n", keys.join("\n"));
    std::fs::write(PathBuf::from(gh.dir()).join("hang-keys"), text)?;
    Ok(())
}

pub(crate) fn rendezvous(gh: &Gh, calls: usize) -> Result<(), Box<dyn Error>> {
    let dir = PathBuf::from(gh.dir());
    let markers = dir.join("rv");
    match std::fs::remove_dir_all(&markers) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    std::fs::create_dir_all(&markers)?;
    std::fs::write(dir.join("rendezvous"), format!("{calls}\n"))?;
    Ok(())
}

pub(crate) fn unit_scope(unit: StateUnit, root: &str) -> Result<StateScope, Box<dyn Error>> {
    Ok(StateScope {
        trust: StateTrust::Trusted,
        unit,
        root: RootSlug::from_root(root)?,
    })
}

pub(crate) fn unit_digest(seed: u64) -> String {
    format!("s256-{seed:064x}")
}

pub(crate) fn unit_key(
    unit: StateUnit,
    root: &str,
    ordinal: u64,
    snapshot: u64,
    payload: u64,
) -> Result<String, Box<dyn Error>> {
    Ok(format!(
        "{}{}-{}-{}",
        unit_scope(unit, root)?.prefix(),
        unit_compat(ordinal),
        unit_digest(snapshot),
        unit_digest(payload)
    ))
}

pub(crate) fn unit_compat(ordinal: u64) -> String {
    unit_digest(0x100 + ordinal)
}

pub(crate) fn unit_snapshot(ordinal: u64) -> String {
    unit_digest(0x1000 + ordinal)
}

pub(crate) fn run_select_units(
    scene: &Scene,
    gh: &Gh,
    units: &[(StateUnit, &str)],
    extra: &[(&str, &str)],
) -> Result<String, Box<dyn Error>> {
    match std::fs::remove_file(&scene.github_output) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let roots = units
        .iter()
        .map(|(unit, root)| {
            Ok(SealRoot {
                unit: *unit,
                slug: RootSlug::from_root(root)?,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let script = script_of(&state_select_step(&roots, JobTimeout::PLAN)?)?;
    let dir = gh.dir();
    let mut env: Vec<(String, String)> = vec![
        ("FAKE_GH_DIR".to_owned(), dir),
        ("GITHUB_REPOSITORY".to_owned(), REPO.to_owned()),
        (
            "GITHUB_WORKFLOW_REF".to_owned(),
            OTHER_WORKFLOW_REF.to_owned(),
        ),
        ("GH_TOKEN".to_owned(), "fixture".to_owned()),
    ];
    for ordinal in 0..units.len() as u64 {
        env.push((
            format!("VELNOR_STATE_COMPAT_R{ordinal}"),
            unit_compat(ordinal),
        ));
        env.push((
            format!("VELNOR_STATE_SNAPSHOT_R{ordinal}"),
            unit_snapshot(ordinal),
        ));
    }
    env.extend(
        extra
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    );
    let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    scene.run_script(&script, &pairs, None)
}

pub(crate) fn selected_unit(
    scene: &Scene,
    unit: StateUnit,
    root: &str,
) -> Result<Vec<String>, Box<dyn Error>> {
    let slug = RootSlug::from_root(root)?;
    scene.output_block(&state_attested_output(unit.as_str(), slug.as_str()))
}

pub(crate) fn produce_flag(
    scene: &Scene,
    unit: StateUnit,
    root: &str,
) -> Result<String, Box<dyn Error>> {
    let slug = RootSlug::from_root(root)?;
    scene.output_value(&state_produce_output(unit.as_str(), slug.as_str()))
}

pub(crate) fn expect(got: &[String], want: &[String], label: &str) -> Result<(), Box<dyn Error>> {
    if got == want {
        Ok(())
    } else {
        Err(fail(&format!("{label}: selected {got:?}, wanted {want:?}")))
    }
}

pub(crate) fn expect_flag(got: &str, want: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if got == want {
        Ok(())
    } else {
        Err(fail(&format!("{label}: produce {got}, wanted {want}")))
    }
}

pub(crate) fn at(second: usize) -> String {
    let total = 86_399 - second;
    format!(
        "2026-11-04T{:02}:{:02}:{:02}Z",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

pub(crate) fn rows_of(rows: &[(String, String)]) -> Vec<(&str, &str, &str)> {
    rows.iter()
        .map(|(created, key)| (created.as_str(), MAIN, key.as_str()))
        .collect()
}
