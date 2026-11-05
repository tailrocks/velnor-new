// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_contract::workflow::cache_family::{
    RootSlug, S256, SLICE_ID_MAX_BYTES, SliceId, StateEntryKey, StateIdentity, StateScope,
    StateTrust, StateUnit, is_plan_cache_output, is_state_snapshot_output,
};
use velnor_actions_contract::workflow::state_seal::{
    is_state_attested_output, is_state_compat_output, is_state_matrix_output,
};

const DIGEST_DELIMITER: &str = "-s256-";

type Accepts = fn(&str) -> bool;

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn slice(id: &str) -> Result<StateUnit, Box<dyn Error>> {
    Ok(StateUnit::Slice(SliceId::parse(id)?))
}

fn adversarial_units() -> Result<Vec<(&'static str, StateUnit)>, Box<dyn Error>> {
    Ok(vec![
        ("slice a", slice("a")?),
        ("slice a-b", slice("a-b")?),
        ("slice a-", slice("a-")?),
        ("cli", StateUnit::Cli),
        ("slice cli", slice("cli")?),
        ("slice suite-a", slice("suite-a")?),
        ("suite a", StateUnit::Suite(RootSlug::from_root("a")?)),
        ("slice 48 a", slice(&"a".repeat(SLICE_ID_MAX_BYTES))?),
        (
            "slice 47 a and b",
            slice(&format!("{}b", "a".repeat(SLICE_ID_MAX_BYTES - 1)))?,
        ),
    ])
}

fn key_of(unit: &StateUnit, compat: S256) -> StateEntryKey {
    StateEntryKey {
        identity: StateIdentity {
            scope: StateScope {
                trust: StateTrust::Trusted,
                unit: unit.clone(),
            },
            compat,
            snapshot: S256::of(b"snapshot"),
        },
        payload: S256::of(b"payload"),
    }
}

#[test]
fn no_unit_restore_prefix_matches_a_key_of_another_unit() -> Result<(), Box<dyn Error>> {
    let compat = S256::of(b"compat");
    let units = adversarial_units()?;
    for (own_label, own) in &units {
        let own_key = key_of(own, compat);
        let own_prefix = own_key.identity.scope.restore_prefix(&compat);
        if !own_key.render().starts_with(&own_prefix) {
            return Err(fail(&format!("{own_label} lost its own restore prefix")));
        }
        for (other_label, other) in &units {
            if own == other {
                continue;
            }
            let other_key = key_of(other, compat).render();
            if other_key.starts_with(&own_prefix) {
                return Err(fail(&format!(
                    "restore prefix of {own_label} ({own_prefix}) matches the key of {other_label} ({other_key})"
                )));
            }
        }
    }
    Ok(())
}

#[test]
fn every_unit_renders_a_distinct_four_segment_key() -> Result<(), Box<dyn Error>> {
    let compat = S256::of(b"compat");
    let units = adversarial_units()?;
    let mut scopes: Vec<String> = Vec::new();
    for (label, unit) in &units {
        let key = key_of(unit, compat).render();
        let segments: Vec<&str> = key.split(DIGEST_DELIMITER).collect();
        let [scope, ..] = segments[..] else {
            return Err(fail(&format!("{label} rendered no scope segment: {key}")));
        };
        if segments.len() != 4 {
            return Err(fail(&format!(
                "{label} rendered {} digest-delimited segments, wanted 4: {key}",
                segments.len()
            )));
        }
        if scopes.iter().any(|seen| seen == scope) {
            return Err(fail(&format!("{label} shares its scope segment {scope}")));
        }
        scopes.push(scope.to_owned());
    }
    Ok(())
}

#[test]
fn a_slice_id_with_a_digest_segment_never_forges_a_key_shape() -> Result<(), Box<dyn Error>> {
    let compat = S256::of(b"compat");
    let forged_tail =
        &compat.to_string()["s256-".len()..][..SLICE_ID_MAX_BYTES - "a-s256-".len() - 1];
    for id in [
        format!("a-s256-{forged_tail}"),
        "a-s256-b".to_owned(),
        "s256-a".to_owned(),
    ] {
        match SliceId::parse(&id) {
            Err(_) => {}
            Ok(parsed) => {
                let key = key_of(&StateUnit::Slice(parsed), compat).render();
                let segments = key.split(DIGEST_DELIMITER).count();
                return Err(fail(&format!(
                    "slice id {id} was accepted and renders {segments} segments instead of 4: {key}"
                )));
            }
        }
    }
    Ok(())
}

#[test]
fn slice_id_rejects_anything_outside_lowercase_digits_and_hyphen() -> Result<(), Box<dyn Error>> {
    for bad in [
        "", "-a", "A", "aB", "a_b", "a/b", "a'b", "a b", "a\tb", "a\nb", " a", "a ", "é", "aé",
        "a.b", "s256", "a-s256", "s256-a", "a-s256-b",
    ] {
        if SliceId::parse(bad).is_ok() {
            return Err(fail(&format!("slice id {bad:?} was accepted")));
        }
    }
    if SliceId::parse(&"a".repeat(SLICE_ID_MAX_BYTES + 1)).is_ok() {
        return Err(fail("a slice id over the byte cap was accepted"));
    }
    for good in [
        "a",
        "a-b",
        "0",
        "a-s256x",
        "x-s25",
        &"a".repeat(SLICE_ID_MAX_BYTES),
    ] {
        SliceId::parse(good)
            .map_err(|error| fail(&format!("slice id {good:?} refused: {error}")))?;
    }
    Ok(())
}

#[test]
fn slice_id_keeps_trailing_hyphens_as_distinct_ids() -> Result<(), Box<dyn Error>> {
    let base = SliceId::parse("a")?;
    for id in ["a-", "a--"] {
        let parsed = SliceId::parse(id).map_err(|error| fail(&format!("{id} refused: {error}")))?;
        if parsed.as_str() != id || parsed == base {
            return Err(fail(&format!("{id} parsed as {parsed}")));
        }
    }
    Ok(())
}

#[test]
fn plan_output_names_accept_only_slice_cli_and_suite_units() -> Result<(), Box<dyn Error>> {
    let kinds: [(&str, Accepts); 3] = [
        ("attested", is_state_attested_output),
        ("compat", is_state_compat_output),
        ("snapshot", is_state_snapshot_output),
    ];
    for (kind, accepts) in kinds {
        for unit in [
            "slice_a",
            "slice_a_b",
            "slice_cli",
            "slice_suite_a",
            "cli",
            "suite_a",
        ] {
            let name = format!("state_{unit}_{kind}");
            if !accepts(&name) || !is_plan_cache_output(&name) {
                return Err(fail(&format!("{name} was refused")));
            }
        }
        for unit in [
            "union_a",
            "a",
            "slice_",
            "slice",
            "suite_",
            "suite",
            "cli_a",
            "Slice_a",
            "slice_a-b",
            "slice a",
        ] {
            let name = format!("state_{unit}_{kind}");
            if accepts(&name) || is_plan_cache_output(&name) {
                return Err(fail(&format!("{name} was accepted")));
            }
        }
    }
    for name in ["state_union_a_snapshot", "state_a_snapshot"] {
        if is_state_snapshot_output(name) || is_plan_cache_output(name) {
            return Err(fail(&format!("{name} was accepted")));
        }
    }
    for name in ["state_matrix_seal", "state_matrix_producer_a_b"] {
        if !is_state_matrix_output(name) || !is_plan_cache_output(name) {
            return Err(fail(&format!("{name} was refused")));
        }
    }
    for name in [
        "state_matrix_producer_a-b",
        "state_matrix_producer_",
        "state_matrix_producer_A",
        "state_matrix_seal_a",
    ] {
        if is_state_matrix_output(name) || is_plan_cache_output(name) {
            return Err(fail(&format!("{name} was accepted")));
        }
    }
    Ok(())
}
