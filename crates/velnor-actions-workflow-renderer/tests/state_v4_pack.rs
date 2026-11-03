// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::process::Command;

#[cfg(target_os = "linux")]
use velnor_actions_contract::workflow::cache_family::MBX_COMPILE_ENV;

use crate::gc1_mbx_stub::EXPORT_MARK_ENV;
use crate::gc1_scene::{Key, Scene, fail};

const PREDECESSOR_MARK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn payload_digest(scene: &Scene) -> Result<String, Box<dyn Error>> {
    let output = Command::new("sha256sum").arg(scene.payload()).output()?;
    let text = String::from_utf8(output.stdout)?;
    text.split_whitespace()
        .next()
        .map(|digest| format!("s256-{digest}"))
        .ok_or_else(|| fail("sha256sum printed nothing"))
}

fn payload_len(scene: &Scene) -> Result<u64, Box<dyn Error>> {
    Ok(std::fs::metadata(scene.payload())?.len())
}

fn fallback_scene(name: &str) -> Result<(Scene, Key, String, u64), Box<dyn Error>> {
    let scene = Scene::new(name, crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.mark_producer()?;
    scene.export(&[(EXPORT_MARK_ENV, PREDECESSOR_MARK)])?;
    let predecessor_len = payload_len(&scene)?;
    let matched = format!("{}oldsnap-0", key.restore_prefix);
    scene.recover(&matched, &[], None)?;
    scene.mark_producer()?;
    Ok((scene, key, matched, predecessor_len))
}

#[test]
#[cfg(target_os = "linux")]
fn export_saves_every_new_generation_regardless_of_bytes() -> Result<(), Box<dyn Error>> {
    let grown: String = (0..255_u8)
        .map(|n| char::from(b'a' + n.wrapping_mul(7) % 26))
        .collect();
    for (label, shape) in [
        ("fewer", std::cmp::Ordering::Less),
        ("equal", std::cmp::Ordering::Equal),
        ("more", std::cmp::Ordering::Greater),
    ] {
        let (scene, key, matched, predecessor_len) = fallback_scene(&format!("v4-pack-{label}"))?;
        let candidates: Vec<String> = match shape {
            std::cmp::Ordering::Less => vec!["x".to_owned()],
            std::cmp::Ordering::Greater => vec![grown.clone()],
            std::cmp::Ordering::Equal => ('b'..='z')
                .map(|c| c.to_string().repeat(PREDECESSOR_MARK.len()))
                .collect(),
        };
        let mut size = 0;
        for mark in &candidates {
            let output = scene.export(&[(EXPORT_MARK_ENV, mark.as_str())])?;
            if scene.output_value("exported")? != "true" {
                return Err(fail(&format!(
                    "{label}: export refused a new generation: {output}"
                )));
            }
            size = payload_len(&scene)?;
            if size.cmp(&predecessor_len) == shape {
                break;
            }
        }
        if size.cmp(&predecessor_len) != shape {
            return Err(fail(&format!(
                "{label}: no fixture reached the intended order ({size} vs {predecessor_len})"
            )));
        }
        let saved = scene.output_value("key")?;
        if saved == matched
            || !saved.starts_with(&key.snapshot_prefix)
            || !saved.ends_with(&payload_digest(&scene)?)
        {
            return Err(fail(&format!(
                "{label}: save key {saved} does not bind its payload"
            )));
        }
        scene.finish()?;
    }
    let scene = Scene::new("v4-pack-exact", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export(PREDECESSOR_MARK)?;
    scene.recover(&key.exact(), &[], None)?;
    scene.mark_producer()?;
    let output = scene.export(&[(EXPORT_MARK_ENV, "changed")])?;
    if !output.contains("not published (exact_hit)") {
        return Err(fail(&format!("exact hit was not skipped: {output}")));
    }
    if scene.output_value("exported")? != "false" || scene.output_value("key").is_ok() {
        return Err(fail("exact hit must publish exported=false and no key"));
    }
    scene.finish()
}

#[test]
#[cfg(target_os = "linux")]
fn publish_refuses_off_table_env() -> Result<(), Box<dyn Error>> {
    for (name, value) in MBX_COMPILE_ENV {
        let scene = Scene::new("v4-env-gate", crate::gc1_t53::scripts()?)?;
        scene.discover()?;
        scene.mark_producer()?;
        let flipped = if value == "0" { "1" } else { "0" };
        let output = scene.export(&[(name, flipped)])?;
        if !output.contains("not published (env_mismatch)")
            || scene.output_value("exported")? != "false"
        {
            return Err(fail(&format!("{name}={flipped} was not refused: {output}")));
        }
        scene.finish()?;
    }
    let scene = Scene::new("v4-env-admit", crate::gc1_t53::scripts()?)?;
    scene.discover()?;
    scene.mark_producer()?;
    scene.export(&[])?;
    if scene.output_value("exported")? != "true" {
        return Err(fail("a matching env must be admitted"));
    }
    scene.finish()
}
