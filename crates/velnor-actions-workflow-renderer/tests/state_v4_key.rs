// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES;
use velnor_actions_contract::workflow::cache_family::{CacheFamily, MBX_COMPILE_ENV};
use velnor_actions_workflow_renderer::steps::MbxTransport;

use crate::gc1_mbx_stub::{UNAME_M_ENV, UNAME_S_ENV};
use crate::gc1_scene::{DEFAULT_SNAPSHOT, Key, Scene, fail};

const KEY_PREFIX: &str = "velnor-v5-state-trusted-slice-root-";
const OTHER_SNAPSHOT: &str =
    "s256-3333333333333333333333333333333333333333333333333333333333333333";

struct Segments {
    scope: String,
    compat: String,
    snapshot: String,
}

fn segments(key: &str) -> Result<Segments, Box<dyn Error>> {
    let parts: Vec<&str> = key.split("-s256-").collect();
    let [scope, compat, snapshot] = parts[..] else {
        return Err(fail(&format!("key lost its two digest segments: {key}")));
    };
    let snapshot = snapshot
        .strip_suffix('-')
        .ok_or_else(|| fail(&format!("lookup key must end with a dash: {key}")))?;
    for digest in [compat, snapshot] {
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(fail(&format!("digest segment malformed in {key}")));
        }
    }
    Ok(Segments {
        scope: scope.to_owned(),
        compat: compat.to_owned(),
        snapshot: snapshot.to_owned(),
    })
}

fn keyed(scene: &Scene, env: &[(&str, &str)]) -> Result<Key, Box<dyn Error>> {
    scene.discover_with(env)
}

fn compat_of(scene: &Scene, env: &[(&str, &str)]) -> Result<String, Box<dyn Error>> {
    Ok(segments(&keyed(scene, env)?.snapshot_prefix)?.compat)
}

#[test]
#[cfg(unix)]
fn state_key_is_stable_and_binds_every_input() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("v4-key", crate::gc1_t53::scripts()?)?;
    let first = keyed(&scene, &[])?;
    let again = keyed(&scene, &[])?;
    let other_run = keyed(
        &scene,
        &[
            ("GITHUB_RUN_ID", "2002"),
            ("GITHUB_RUN_ATTEMPT", "3"),
            ("RUNNER_NAME", "runner-b"),
        ],
    )?;
    for other in [&again, &other_run] {
        if first.snapshot_prefix != other.snapshot_prefix
            || first.restore_prefix != other.restore_prefix
        {
            return Err(fail("key changed without an input change"));
        }
    }
    let parts = segments(&first.snapshot_prefix)?;
    if !first.snapshot_prefix.starts_with(KEY_PREFIX)
        || parts.scope != KEY_PREFIX.trim_end_matches('-')
    {
        return Err(fail(&format!(
            "key lost its v5 scope shape: {}",
            first.snapshot_prefix
        )));
    }
    if !first.restore_prefix.starts_with(KEY_PREFIX)
        || !first.snapshot_prefix.starts_with(&first.restore_prefix)
        || first.snapshot_prefix != format!("{}{DEFAULT_SNAPSHOT}-", first.restore_prefix)
    {
        return Err(fail(
            "snapshot prefix is not the compat prefix plus the snapshot",
        ));
    }
    if parts.snapshot != DEFAULT_SNAPSHOT.trim_start_matches("s256-") {
        return Err(fail("the snapshot segment is not the supplied snapshot"));
    }
    let baseline = parts.compat;
    for (label, env) in [
        ("ImageOS", vec![("ImageOS", "ubuntu24")]),
        ("uname -s", vec![(UNAME_S_ENV, "StubOS")]),
        ("uname -m", vec![(UNAME_M_ENV, "stubarch")]),
        ("mbx version", vec![("VELNOR_MBX_BUNDLE_VERSION", "9.9.9")]),
        ("toolchain", vec![("VELNOR_MBX_BUNDLE_TOOLCHAIN", "1.90.0")]),
        ("root", vec![("VELNOR_MBX_BUNDLE_ROOT", "crates/other")]),
    ] {
        if compat_of(&scene, &env)? == baseline {
            return Err(fail(&format!("compat ignored {label}")));
        }
    }
    let suite_script = crate::gc1_t53::scripts_for(MbxTransport::SuiteBundle, None)?.discover;
    let suite_key = scene.discover_script(&suite_script)?;
    if !suite_key
        .snapshot_prefix
        .starts_with("velnor-v5-state-trusted-suite-root-")
        || segments(&suite_key.snapshot_prefix)?.compat == baseline
    {
        return Err(fail("suite unit shares the slice key"));
    }
    let second = Scene::new("v4-key-paths", crate::gc1_t53::scripts()?)?;
    if compat_of(&second, &[])? == baseline {
        return Err(fail("compat ignored the store and checkout paths"));
    }
    second.finish()?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn state_key_slugs_roots_and_stays_inside_the_cache_cap() -> Result<(), Box<dyn Error>> {
    let runner = Scene::new(
        "v4-key-root",
        crate::gc1_t53::scripts_for(
            MbxTransport::SeedRead(crate::gc1_seal_support::slice_of("crates/velnor-runner")?),
            Some("crates/velnor-runner"),
        )?,
    )?;
    let key = keyed(
        &runner,
        &[("VELNOR_MBX_BUNDLE_ROOT", "crates/velnor-runner")],
    )?;
    if !key
        .snapshot_prefix
        .starts_with("velnor-v5-state-trusted-slice-crates-velnor-runner-s256-")
    {
        return Err(fail(&format!(
            "runner root lost its slug: {}",
            key.snapshot_prefix
        )));
    }
    let widest_root = "d".repeat(48);
    let widest = Scene::new(
        "v4-key-widest",
        crate::gc1_t53::scripts_for(
            MbxTransport::SeedRead(crate::gc1_seal_support::slice_of(&widest_root)?),
            Some(&widest_root),
        )?,
    )?;
    let long = keyed(&widest, &[("VELNOR_MBX_BUNDLE_ROOT", &widest_root)])?;
    let entry = long.exact().len();
    if entry > MAX_CACHE_KEY_BYTES || entry > CacheFamily::MbxBundle.max_key_bytes() {
        return Err(fail(&format!(
            "entry key of {entry} bytes outgrew the caps"
        )));
    }
    widest.finish()?;
    runner.finish()
}

#[test]
#[cfg(unix)]
fn state_key_ignores_literal_env() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("v4-literal", crate::gc1_t53::scripts()?)?;
    let baseline = keyed(&scene, &[])?;
    for (name, value) in MBX_COMPILE_ENV {
        let flipped = if value == "0" { "1" } else { "0" };
        for env in [vec![(name, flipped)], vec![(name, "")]] {
            let key = keyed(&scene, &env)?;
            if key.snapshot_prefix != baseline.snapshot_prefix
                || key.restore_prefix != baseline.restore_prefix
            {
                return Err(fail(&format!("{name} reached the key as {env:?}")));
            }
        }
        let outputs = scene.discover_outputs(&[], &[name])?;
        if !outputs
            .lines()
            .any(|line| line == format!("key={}", baseline.snapshot_prefix))
        {
            return Err(fail(&format!("{name} unset reached the key")));
        }
    }
    scene.finish()
}

fn skip_of(scene: &Scene, matched: &str) -> Result<(String, String), Box<dyn Error>> {
    scene.recover(matched, &[], None)?;
    let skip = scene.output_value("producer_skip")?;
    let record = std::fs::read_to_string(scene.recovery_record()).unwrap_or_default();
    let decision = record
        .lines()
        .find_map(|line| line.strip_prefix("decision="))
        .unwrap_or("none")
        .to_owned();
    drop(std::fs::remove_file(scene.recovery_record()));
    Ok((skip, decision))
}

#[test]
#[cfg(target_os = "linux")]
fn recover_sets_producer_skip_only_on_exact_snapshot() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("v4-skip", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    let other_snapshot = format!(
        "{}{OTHER_SNAPSHOT}-s256-{}",
        key.restore_prefix,
        "4".repeat(64)
    );
    scene.pack_export("held")?;
    let exact = skip_of(&scene, &key.exact())?;
    if exact != ("true".to_owned(), "exact-hit".to_owned()) {
        return Err(fail(&format!("exact snapshot hit gave {exact:?}")));
    }
    scene.pack_export("held")?;
    let fallback = skip_of(&scene, &other_snapshot)?;
    if fallback != ("false".to_owned(), "predecessor-fallback".to_owned()) {
        return Err(fail(&format!("other snapshot gave {fallback:?}")));
    }
    let cold = skip_of(&scene, "")?;
    if cold.0 != "false" {
        return Err(fail(&format!("cold recover gave {cold:?}")));
    }
    scene.pack_export("held")?;
    let refused = skip_of(&scene, "velnor-v3-state-trusted-foreign")?;
    if refused != ("false".to_owned(), "none".to_owned()) {
        return Err(fail(&format!("refused recover gave {refused:?}")));
    }
    scene.finish()
}
