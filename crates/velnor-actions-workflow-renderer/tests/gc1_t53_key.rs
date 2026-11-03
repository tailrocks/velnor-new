// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_MATCHED_KEY_EXPR;

use crate::gc1_scene::Scene;

use super::*;

const REFUSED: &str = "refused-provenance";

fn injection(scene: &Scene) -> String {
    format!("\"; touch {}/INJECTED; echo \"", scene.scratch.display())
}

fn check_key(name: &str, build: fn(&str, &str) -> String) -> Result<(), Box<dyn Error>> {
    let scene = Scene::new(name, crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    let matched = build(&key.restore_prefix, &injection(&scene));
    let before = scene.tree()?;
    let output = scene.recover(&matched, &[], None)?;
    if scene.scratch.join("INJECTED").exists() {
        return Err(fail("the matched key ran as shell code"));
    }
    scene.assert_cold(&output, REFUSED)?;
    scene.assert_confined(&before)?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_ignores_code_in_a_restored_prefix_key() -> Result<(), Box<dyn Error>> {
    check_key("key-prefix", |prefix, code| format!("{prefix}{code}"))
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_survives_a_quote_in_a_restored_prefix_key() -> Result<(), Box<dyn Error>> {
    check_key("key-quote", |prefix, code| format!("{prefix}{code}'"))
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_ignores_code_in_a_foreign_key() -> Result<(), Box<dyn Error>> {
    check_key("key-foreign", |_, code| format!("x{code}"))
}

#[test]
fn gc1_t53_recover_receives_the_matched_key_only_through_env() -> Result<(), Box<dyn Error>> {
    let scripts = crate::gc1_t53::scripts()?;
    if scripts.recover.run.contains("${{") {
        return Err(fail("the recover run text still carries an expression"));
    }
    let carriers = scripts
        .recover
        .env
        .iter()
        .filter(|(_, value)| value == MBX_BUNDLE_MATCHED_KEY_EXPR)
        .count();
    if carriers != 1 {
        return Err(fail(&format!(
            "the matched key expression sits in {carriers} env values"
        )));
    }
    Ok(())
}
