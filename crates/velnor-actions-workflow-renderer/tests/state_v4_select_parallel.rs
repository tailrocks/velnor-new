// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_family::{
    S256, SliceId, StateEntryKey, StateIdentity, StateScope, StateTrust, StateUnit,
};
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_SELECT_PARALLELISM;

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{Attestation, Gh, MAIN, digest, run_select_units, selected_unit};

const T3: &str = "2026-11-04T03:00:00Z";

fn unit_key(unit: &StateUnit) -> StateEntryKey {
    StateEntryKey {
        identity: StateIdentity {
            scope: StateScope {
                trust: StateTrust::Trusted,
                unit: unit.clone(),
            },
            compat: S256::of(b"compat"),
            snapshot: S256::of(b"snapshot"),
        },
        payload: S256::of(unit.token().as_bytes()),
    }
}

#[test]
#[cfg(unix)]
fn plan_verifies_every_unit_concurrently_and_keeps_each_block_to_its_own_key()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::new("seal-select-parallel", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let slice = |id: &str| -> Result<StateUnit, Box<dyn Error>> {
        Ok(StateUnit::Slice(SliceId::parse(id)?))
    };
    let mut units = vec![
        slice("a")?,
        slice("a-b")?,
        slice("a-")?,
        slice("cli")?,
        StateUnit::Cli,
    ];
    for ordinal in 0..STATE_SEAL_SELECT_PARALLELISM - units.len() {
        units.push(slice(&format!("u{ordinal}"))?);
    }
    expect_count(units.len(), STATE_SEAL_SELECT_PARALLELISM)?;

    let own: Vec<String> = units.iter().map(|unit| unit_key(unit).render()).collect();
    let neighbour = format!(
        "{}extra-{}-{}-{}",
        unit_key(&units[0]).identity.scope.prefix(),
        digest('a'),
        digest('b'),
        digest('c')
    );
    let mut rows: Vec<(&str, &str, &str)> =
        own.iter().map(|key| (T3, MAIN, key.as_str())).collect();
    rows.push((T3, MAIN, neighbour.as_str()));
    gh.list(&rows)?;
    let mut attested: Vec<Attestation> = own.iter().map(|key| Attestation::signed(key)).collect();
    attested.push(Attestation::signed(&neighbour));
    gh.attest(&attested)?;
    gh.rendezvous(units.len())?;

    run_select_units(&scene, &gh, &units, &[])?;

    let (reached, gave_up) = gh.rendezvous_outcome();
    if !reached || gave_up {
        return Err(fail(&format!(
            "the {} unit verifications never ran at the same time (reached {reached}, gave up {gave_up})",
            units.len()
        )));
    }
    for (unit, key) in units.iter().zip(&own) {
        let got = selected_unit(&scene, unit)?;
        if got != [key.as_str()] {
            return Err(fail(&format!(
                "block of {unit}: selected {got:?}, wanted {key:?}"
            )));
        }
    }
    scene.finish()
}

fn expect_count(got: usize, want: usize) -> Result<(), Box<dyn Error>> {
    if got == want {
        Ok(())
    } else {
        Err(fail(&format!("fixture holds {got} units, wanted {want}")))
    }
}
