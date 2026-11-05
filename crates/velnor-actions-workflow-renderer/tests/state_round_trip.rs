// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::PathBuf;

use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_ARTIFACT_ENV, STATE_SEAL_KEY_ENV, STATE_SEAL_PREFIX_ENV, STATE_SEAL_SCOPE_ENV,
};
use velnor_actions_contract::{Job, StepRole};

use crate::gc1_mbx_stub::EXPORT_MARK_ENV;
use crate::gc1_scene::{DEFAULT_SNAPSHOT, Scene, fail};
use crate::gc1_seal_support::{
    Attestation, Gh, MAIN, OTHER_WORKFLOW_REF, REPO, digest, run_select, scope_of, selected,
};
use crate::state_v4_attest_save::script_and_env;
use crate::state_v4_bind::run_bind;
use crate::state_v4_seal::seal_job;

const AT: &str = "2026-11-04T01:00:00Z";
const ROOT: &str = ".";
const PRODUCE: &str = "state_slice_root_produce";

struct Cache {
    dir: PathBuf,
}

fn tail_of<'a>(prefix: &'a str, scope: &str) -> Result<&'a str, Box<dyn Error>> {
    prefix
        .strip_prefix(scope)
        .ok_or_else(|| fail("prefix outside the scope"))
}

fn fresh_runner(scene: &Scene) -> Result<(), Box<dyn Error>> {
    for dir in [scene.store(), scene.runner.join("velnor-gc1")] {
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
    }
    std::fs::create_dir_all(scene.store().join("actions"))?;
    for file in ["state-recovery.record", "state-producer-ok"] {
        drop(std::fs::remove_file(scene.runner.join("velnor").join(file)));
    }
    Ok(())
}

fn plan(scene: &Scene, gh: &Gh, prefix: &str, scope: &str) -> Result<String, Box<dyn Error>> {
    let tail = tail_of(prefix, scope)?;
    let (compat, snapshot) = (&tail[..69], &tail[70..139]);
    run_select(
        scene,
        gh,
        ROOT,
        &[
            ("VELNOR_STATE_COMPAT_R0", compat),
            ("VELNOR_STATE_SNAPSHOT_R0", snapshot),
        ],
    )?;
    scene.output_value(PRODUCE)
}

fn seal(
    scene: &Scene,
    gh: &Gh,
    cache: &Cache,
    job: &Job,
    prefix: &str,
    id: u64,
) -> Result<String, Box<dyn Error>> {
    let scope = scope_of(ROOT)?.prefix();
    let (script, env) = script_and_env(job, StepRole::StateSealSubject)?;
    let mut pairs: Vec<(&str, &str)> = env
        .iter()
        .filter(|(name, _)| name != STATE_SEAL_PREFIX_ENV)
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    pairs.push((STATE_SEAL_PREFIX_ENV, prefix));
    scene.run_script(&script, &pairs, None)?;
    let key = scene.output_value("key")?;
    let size = std::fs::metadata(scene.payload())?.len();
    std::fs::copy(scene.payload(), cache.dir.join(id.to_string()))?;
    let mut entries = gh.rows()?;
    entries.push((key.clone(), id));
    let rows: Vec<(&str, &str, &str, u64, u64)> = entries
        .iter()
        .map(|(entry, entry_id)| {
            let entry_size = if *entry_id == id { size } else { 1 };
            (AT, MAIN, entry.as_str(), *entry_id, entry_size)
        })
        .collect();
    gh.list_entries(&rows)?;
    gh.artifacts(&[7001])?;
    run_bind(scene, gh, job, &key)?;
    gh.attest(&[Attestation::bound(&key, id, size)])?;
    let (script, env) = script_and_env(job, StepRole::StateSealRetire)?;
    let dir = gh.dir();
    let mut pairs: Vec<(&str, &str)> = env
        .iter()
        .filter(|(name, _)| {
            ![
                STATE_SEAL_KEY_ENV,
                STATE_SEAL_ARTIFACT_ENV,
                STATE_SEAL_SCOPE_ENV,
                "VELNOR_DEFAULT_REF",
            ]
            .contains(&name.as_str())
        })
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    pairs.extend([
        ("FAKE_GH_DIR", dir.as_str()),
        ("GITHUB_REPOSITORY", REPO),
        ("GITHUB_WORKFLOW_REF", OTHER_WORKFLOW_REF),
        ("GH_TOKEN", "fixture"),
        ("VELNOR_DEFAULT_REF", MAIN),
        (STATE_SEAL_KEY_ENV, key.as_str()),
        (STATE_SEAL_ARTIFACT_ENV, "7001"),
        (STATE_SEAL_SCOPE_ENV, scope.as_str()),
    ]);
    scene.run_script(&script, &pairs, None)?;
    Ok(key)
}

fn restore(scene: &Scene, cache: &Cache, id: u64) -> Result<(), Box<dyn Error>> {
    let path = scene.payload();
    std::fs::create_dir_all(path.parent().ok_or_else(|| fail("no parent"))?)?;
    std::fs::copy(cache.dir.join(id.to_string()), path)?;
    Ok(())
}

struct Runs<'a> {
    scene: &'a Scene,
    gh: &'a Gh,
    cache: &'a Cache,
    job: &'a Job,
    scope: &'a str,
}

fn second_run(runs: &Runs<'_>, first: &str, first_prefix: &str) -> Result<(), Box<dyn Error>> {
    let Runs {
        scene,
        gh,
        cache,
        scope,
        ..
    } = runs;
    fresh_runner(scene)?;
    if plan(scene, gh, first_prefix, scope)? != "false"
        || selected(scene, ROOT)? != [first.to_owned()]
    {
        return Err(fail(
            "run 2 must select the attested entry and skip the producer",
        ));
    }
    scene.discover_with(&[("VELNOR_STATE_ATTESTED", first)])?;
    if scene.output_value("restore_key")? != first || scene.output_value("exact")? != "true" {
        return Err(fail("run 2 must restore the attested key exactly"));
    }
    restore(scene, cache, 20)?;
    scene.recover_selected(first, first, &[], None)?;
    let record = std::fs::read_to_string(scene.recovery_record())?;
    if !record.contains("decision=exact-hit")
        || scene.store_files()?.is_empty()
        || scene.output_value("producer_skip")? != "true"
    {
        return Err(fail(&format!(
            "run 2 did not import the first run's state: {record}"
        )));
    }

    Ok(())
}

fn third_run(
    runs: &Runs<'_>,
    first: &str,
    first_prefix: &str,
) -> Result<(String, String), Box<dyn Error>> {
    let Runs {
        scene,
        gh,
        cache,
        job,
        scope,
    } = runs;
    fresh_runner(scene)?;
    let changed = digest('3');
    let second_prefix = format!(
        "{}{changed}-",
        &first_prefix[..first_prefix.len() - DEFAULT_SNAPSHOT.len() - 1]
    );
    if plan(scene, gh, &second_prefix, scope)? != "true" {
        return Err(fail("run 3 must be eligible after a snapshot change"));
    }
    scene.discover_with(&[
        ("VELNOR_STATE_ATTESTED", first),
        ("VELNOR_STATE_SNAPSHOT", changed.as_str()),
    ])?;
    if scene.output_value("restore_key")? != first || scene.output_value("exact")? != "false" {
        return Err(fail("run 3 must fall back to the predecessor"));
    }
    restore(scene, cache, 20)?;
    scene.recover_selected(first, first, &[], None)?;
    let record = std::fs::read_to_string(scene.recovery_record())?;
    if !record.contains("decision=predecessor-fallback")
        || scene.output_value("producer_skip")? != "false"
    {
        return Err(fail(&format!("run 3 lost its warm start: {record}")));
    }
    scene.mark_producer()?;
    scene.export(&[(EXPORT_MARK_ENV, "gen2")])?;
    let second = seal(scene, gh, cache, job, &second_prefix, 21)?;
    if gh
        .rows()?
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        != [second.as_str()]
    {
        return Err(fail("run 3 must leave exactly the new generation"));
    }
    if !gh.log().contains("delete cache 20\n") {
        return Err(fail("run 3 did not delete the superseded generation"));
    }

    Ok((second, second_prefix))
}

#[test]
fn second_run_restores_what_the_first_run_attested() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("round-trip", crate::gc1_t53::scripts()?)?;
    let gh = Gh::install(&scene)?;
    let job = seal_job()?;
    let cache = Cache {
        dir: scene.scratch.join("cache-service"),
    };
    std::fs::create_dir_all(&cache.dir)?;
    let scope = scope_of(ROOT)?.prefix();

    let key = scene.discover()?;
    let first_prefix = key.snapshot_prefix.clone();
    if plan(&scene, &gh, &first_prefix, &scope)? != "true" {
        return Err(fail("a cold repo must be eligible"));
    }
    scene.mark_producer()?;
    scene.export(&[(EXPORT_MARK_ENV, "gen1")])?;
    if scene.output_value("exported")? != "true" {
        return Err(fail("the producer export was refused"));
    }
    let first = seal(&scene, &gh, &cache, &job, &first_prefix, 20)?;
    if gh
        .rows()?
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        != [first.as_str()]
    {
        return Err(fail("run 1 must leave exactly one entry"));
    }

    let runs = Runs {
        scene: &scene,
        gh: &gh,
        cache: &cache,
        job: &job,
        scope: &scope,
    };
    second_run(&runs, &first, &first_prefix)?;
    let (_, second_prefix) = third_run(&runs, &first, &first_prefix)?;
    gh.list(&[])?;
    fresh_runner(&scene)?;
    if plan(&scene, &gh, &second_prefix, &scope)? != "true" {
        return Err(fail("an evicted entry must make the next run eligible"));
    }
    scene.finish()
}
