// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_family::S256;
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_ARTIFACT_ENV, STATE_SEAL_KEY_ENV, STATE_SEAL_SCOPE_ENV, STATE_SEAL_SUBJECT_REL,
};
use velnor_actions_contract::{Job, StepKind, StepRole};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::{
    Attestation, Gh, MAIN, OTHER_WORKFLOW_REF, PULL, REPO, digest, scope_of,
};
use crate::state_v4_bind::run_bind;
use crate::state_v4_seal::{FIXTURE, resolved_prefix, seal_job, step_of};

const OLD_ID: u64 = 11;
const NEW_ID: u64 = 20;
const ARTIFACT: u64 = 7001;
const ARTIFACT_NAME: &str = "velnor-state-root-r7-a1";
const STORED: u64 = 4096;
pub(crate) const BOUND: u64 = 10_339;
const AT: &str = "2026-11-04T01:00:00Z";

pub(crate) type Script = (String, Vec<(String, String)>);

pub(crate) fn script_and_env(job: &Job, role: StepRole) -> Result<Script, Box<dyn Error>> {
    let StepKind::Shell { run, env } = &step_of(job, role)?.kind else {
        return Err(fail("expected a shell step"));
    };
    Ok((
        run.get(2).cloned().ok_or_else(|| fail("no script"))?,
        env.iter()
            .map(|(name, value)| (name.clone(), resolved_prefix(value)))
            .collect(),
    ))
}

struct World {
    scene: Scene,
    gh: Gh,
    job: Job,
    key: String,
    old: String,
    neighbour: String,
    foreign: String,
    suite: String,
    scope: String,
}

impl World {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let scene = Scene::new(name, crate::gc1_t53::scripts()?)?;
        let gh = Gh::install(&scene)?;
        let job = seal_job()?;
        let scope = scope_of(".")?.prefix();
        std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
        std::fs::write(scene.payload(), FIXTURE)?;
        let (script, env) = script_and_env(&job, StepRole::StateSealSubject)?;
        let pairs: Vec<(&str, &str)> = env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        scene.run_script(&script, &pairs, None)?;
        let key = scene.output_value("key")?;
        let tail = format!("{}-{}-{}", digest('a'), digest('b'), digest('c'));
        Ok(Self {
            old: format!("{scope}{tail}"),
            neighbour: format!("{scope}extra-{tail}"),
            foreign: format!("{}{tail}", scope.replacen("-root-", "-other-", 1)),
            suite: format!("{}{tail}", scope.replacen("-slice-", "-suite-", 1)),
            key,
            scope,
            scene,
            gh,
            job,
        })
    }

    fn saved(&self, id: u64, size: u64) -> Result<(), Box<dyn Error>> {
        let mut rows: Vec<(&str, &str, &str, u64, u64)> = vec![
            (AT, MAIN, &self.old, OLD_ID, 1_000_000),
            (AT, MAIN, &self.neighbour, 12, 1),
            (AT, MAIN, &self.foreign, 13, 1),
            (AT, MAIN, &self.suite, 14, 1),
            (AT, PULL, &self.old, 15, 1),
        ];
        if id != 0 {
            rows.push((AT, MAIN, &self.key, id, size));
        }
        self.gh.list_entries(&rows)?;
        self.gh.artifacts(&[ARTIFACT])?;
        self.gh.artifact_name(ARTIFACT_NAME)?;
        self.gh.attest(&[Attestation::bound(&self.key, id, size)])?;
        self.gh.clear_log()
    }

    fn bind(&self) -> Result<(), Box<dyn Error>> {
        run_bind(&self.scene, &self.gh, &self.job, &self.key)
    }

    fn retire(&self) -> Result<String, Box<dyn Error>> {
        let (script, env) = script_and_env(&self.job, StepRole::StateSealRetire)?;
        let dir = self.gh.dir();
        let mut pairs: Vec<(&str, &str)> = env
            .iter()
            .filter(|(name, _)| {
                ![
                    STATE_SEAL_KEY_ENV,
                    STATE_SEAL_ARTIFACT_ENV,
                    STATE_SEAL_SCOPE_ENV,
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
            (STATE_SEAL_KEY_ENV, self.key.as_str()),
            (STATE_SEAL_ARTIFACT_ENV, ARTIFACT_NAME),
            ("GITHUB_RUN_ID", "7"),
            (STATE_SEAL_SCOPE_ENV, self.scope.as_str()),
        ]);
        self.scene.run_script(&script, &pairs, None)
    }

    fn keys(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut keys: Vec<String> = self.gh.rows()?.into_iter().map(|(key, _)| key).collect();
        keys.sort();
        Ok(keys)
    }
}

fn require(cond: bool, message: &str) -> Result<(), Box<dyn Error>> {
    if cond { Ok(()) } else { Err(fail(message)) }
}

#[test]
#[cfg(unix)]
fn new_generation_is_proven_before_the_old_one_is_deleted() -> Result<(), Box<dyn Error>> {
    let world = World::new("attest-save")?;
    let subject = world.scene.runner.join(STATE_SEAL_SUBJECT_REL);
    require(
        world.key.ends_with(&S256::of(FIXTURE).to_string()),
        "the key does not end in the payload digest",
    )?;

    world.saved(NEW_ID, STORED)?;
    world.bind()?;
    let want = format!("{} {NEW_ID} {STORED}\n", world.key);
    require(
        std::fs::read(&subject)? == want.as_bytes(),
        "bound subject is not key id size",
    )?;
    let output = world.retire()?;
    let mut expected = vec![
        world.neighbour.clone(),
        world.foreign.clone(),
        world.suite.clone(),
        world.old.clone(),
        world.key.clone(),
    ];
    expected.sort();
    require(
        world.keys()? == expected,
        &format!(
            "only this root's superseded generation on main may go: {:?} / {output}",
            world.keys()?
        ),
    )?;
    let log = world.gh.log();
    require(
        log.contains(&format!("delete cache {OLD_ID}\n"))
            && log.contains(&format!("delete artifact {ARTIFACT}\n")),
        &format!("old generation and artifact were not deleted: {log}"),
    )?;
    require(
        log.matches("delete cache").count() == 1,
        "more than the superseded generation was deleted",
    )?;
    require(
        output.contains("state snapshot published"),
        "the publication line is missing",
    )?;

    let log_before = world.gh.log();
    world.retire()?;
    require(world.gh.log() == log_before, "a rerun deleted again")?;

    world.saved(0, 0)?;
    world.bind()?;
    require(!subject.exists(), "a missing entry was bound")?;
    world.retire()?;
    require(
        world.gh.log().is_empty() && world.gh.rows()?.iter().any(|(_, id)| *id == OLD_ID),
        "a refused save deleted the old generation or the artifact",
    )?;

    world.saved(NEW_ID, BOUND + 1)?;
    world.bind()?;
    world.retire()?;
    require(
        world.gh.log().is_empty() && world.gh.rows()?.iter().any(|(_, id)| *id == OLD_ID),
        "an oversized entry deleted the old generation",
    )?;

    world.saved(NEW_ID, STORED)?;
    world.gh.attest(&[])?;
    world.bind()?;
    world.retire()?;
    require(
        world.gh.log().is_empty() && world.gh.rows()?.iter().any(|(_, id)| *id == OLD_ID),
        "an unverified attestation deleted the old generation",
    )?;

    world.saved(NEW_ID, STORED)?;
    world.bind()?;
    world.gh.refuse_delete(&[OLD_ID])?;
    let output = world.retire()?;
    require(
        output.contains("could not delete state entry 11")
            && world.keys()?.contains(&world.key)
            && world.gh.log().contains("delete artifact"),
        &format!("a failed delete must warn and keep the new entry: {output}"),
    )?;
    world.scene.finish()
}
