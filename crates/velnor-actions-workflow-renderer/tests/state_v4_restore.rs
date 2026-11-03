// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_KEY_EXPR, MBX_BUNDLE_RESTORE_KEYS_EXPR,
};
use velnor_actions_workflow_renderer::steps::MbxTransport;

use crate::gc1_cache_service::CacheService;
use crate::gc1_scene::{Key, Scene, fail};

const SNAPSHOT_A: &str = "s256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SNAPSHOT_B: &str = "s256-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SNAPSHOT_C: &str = "s256-cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const PAYLOAD_ONE: &str = "s256-1111111111111111111111111111111111111111111111111111111111111111";
const PAYLOAD_TWO: &str = "s256-2222222222222222222222222222222222222222222222222222222222222222";

fn restore(
    scene: &Scene,
    service: &CacheService,
    root: Option<&str>,
) -> Result<Option<String>, Box<dyn Error>> {
    let with = crate::gc1_t53::restore_with(MbxTransport::SeedRead, root)?;
    let key = match with.get("key").map(String::as_str) {
        Some(MBX_BUNDLE_KEY_EXPR) => scene.output_value("key")?,
        other => {
            return Err(fail(&format!(
                "restore key left the discover output: {other:?}"
            )));
        }
    };
    let restore_keys = match with.get("restore-keys").map(String::as_str) {
        Some(MBX_BUNDLE_RESTORE_KEYS_EXPR) => scene.output_block("restore_keys")?,
        other => {
            return Err(fail(&format!(
                "restore-keys left the discover output: {other:?}"
            )));
        }
    };
    Ok(service.restore(&key, &restore_keys))
}

fn discover(scene: &Scene, env: &[(&str, &str)]) -> Result<Key, Box<dyn Error>> {
    scene.discover_with(env)
}

fn at(key: &Key, snapshot: &str, payload: &str) -> String {
    format!("{}{snapshot}-{payload}", key.restore_prefix)
}

fn expect(got: Option<String>, want: &str, label: &str) -> Result<(), Box<dyn Error>> {
    match got {
        Some(picked) if picked == want => Ok(()),
        other => Err(fail(&format!("{label}: picked {other:?}, wanted {want}"))),
    }
}

#[test]
#[cfg(unix)]
fn restore_picks_exact_snapshot_else_newest_same_compat() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("v4-restore", crate::gc1_t53::scripts()?)?;
    let other_compat = discover(&scene, &[("ImageOS", "ubuntu24")])?;
    let key = discover(&scene, &[])?;
    let current = key.snapshot_prefix.trim_end_matches('-');
    let current_snapshot = current
        .rsplit_once("-s256-")
        .map(|(_, tail)| format!("s256-{tail}"))
        .ok_or_else(|| fail("snapshot prefix lost its digest"))?;
    let same_snapshot = format!("{}{PAYLOAD_ONE}", key.snapshot_prefix);
    let same_snapshot_newer = format!("{}{PAYLOAD_TWO}", key.snapshot_prefix);
    let other_old = at(&key, SNAPSHOT_A, PAYLOAD_ONE);
    let other_new = at(&key, SNAPSHOT_B, PAYLOAD_ONE);
    let newest_other = at(&key, SNAPSHOT_C, PAYLOAD_TWO);

    let mut exact = CacheService::new();
    exact.save(&same_snapshot, 10);
    exact.save(&other_new, 20);
    expect(
        restore(&scene, &exact, None)?,
        &same_snapshot,
        "exact preferred over newer",
    )?;

    let mut newest = CacheService::new();
    newest.save(&other_old, 10);
    newest.save(&newest_other, 30);
    newest.save(&other_new, 20);
    expect(
        restore(&scene, &newest, None)?,
        &newest_other,
        "newest same compat",
    )?;

    let mut digests = CacheService::new();
    digests.save(&same_snapshot, 10);
    digests.save(&same_snapshot_newer, 20);
    expect(
        restore(&scene, &digests, None)?,
        &same_snapshot_newer,
        "newest payload digest",
    )?;

    let foreign = [
        key.restore_prefix.replacen("-trusted-", "-pr-", 1),
        key.restore_prefix.replacen("-union-", "-suite-", 1),
        key.restore_prefix.replacen("-root-", "-other-", 1),
        other_compat.restore_prefix.clone(),
        key.restore_prefix.replacen("velnor-v4-", "velnor-v3-", 1),
        key.restore_prefix.replacen("velnor-v4-", "velnor-v2-", 1),
        format!("zz-{}", key.restore_prefix),
    ];
    let legit = at(&key, SNAPSHOT_A, PAYLOAD_ONE);
    let mut hostile = CacheService::new();
    hostile.save(&legit, 1);
    for (offset, prefix) in foreign.iter().enumerate() {
        hostile.save(
            &format!("{prefix}{current_snapshot}-{PAYLOAD_TWO}"),
            100 + offset as u64,
        );
    }
    expect(
        restore(&scene, &hostile, None)?,
        &legit,
        "foreign entries never selected",
    )?;

    let mut only_foreign = CacheService::new();
    for prefix in &foreign {
        only_foreign.save(&format!("{prefix}{current_snapshot}-{PAYLOAD_TWO}"), 5);
    }
    if let Some(picked) = restore(&scene, &only_foreign, None)? {
        return Err(fail(&format!("restore reached a foreign entry: {picked}")));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn restore_never_crosses_roots_that_share_a_name_prefix() -> Result<(), Box<dyn Error>> {
    let short = Scene::new(
        "v4-root-a",
        crate::gc1_t53::scripts_for(MbxTransport::SeedRead, Some("a"))?,
    )?;
    let long = Scene::new(
        "v4-root-ab",
        crate::gc1_t53::scripts_for(MbxTransport::SeedRead, Some("a-b"))?,
    )?;
    let short_key = discover(&short, &[("VELNOR_MBX_BUNDLE_ROOT", "a")])?;
    let long_key = discover(&long, &[("VELNOR_MBX_BUNDLE_ROOT", "a-b")])?;
    let mut service = CacheService::new();
    service.save(&at(&long_key, SNAPSHOT_A, PAYLOAD_ONE), 50);
    if let Some(picked) = restore(&short, &service, Some("a"))? {
        return Err(fail(&format!("root a restored {picked}")));
    }
    service.save(&at(&short_key, SNAPSHOT_A, PAYLOAD_TWO), 1);
    let want = at(&short_key, SNAPSHOT_A, PAYLOAD_TWO);
    expect(
        restore(&short, &service, Some("a"))?,
        &want,
        "root a keeps its own entry",
    )?;
    long.finish()?;
    short.finish()
}
