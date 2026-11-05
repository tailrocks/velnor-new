// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::PathBuf;

use velnor_actions_contract::workflow::cache_family::{
    RootSlug, S256, StateScope, StateTrust, StateUnit,
};
use velnor_actions_contract::workflow::state_seal::{
    STATE_SEAL_PREDICATE_TYPE, state_attested_output,
};
use velnor_actions_contract::{JobTimeout, Step, StepKind};
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealRoot, state_select_step};

use crate::gc1_scene::{Scene, fail};
use gh_script::GH;

#[path = "gc1_seal_gh_script.rs"]
mod gh_script;

pub(crate) const REPO: &str = "tailrocks/velnor-new";
pub(crate) const SIGNER: &str = "tailrocks/velnor-new/.github/workflows/state-seal.yml";
pub(crate) const OTHER_WORKFLOW_REF: &str =
    "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main";
pub(crate) const MAIN: &str = "refs/heads/main";
pub(crate) const PULL: &str = "refs/pull/9/merge";
pub(crate) const ENTRY_ID: u64 = 7001;
pub(crate) const ENTRY_SIZE: u64 = 4096;

pub(crate) const SLEEP_SHIM: &str =
    "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"${FAKE_GH_DIR:?}/sleeps\"\n";

pub(crate) struct Gh {
    dir: PathBuf,
}

pub(crate) struct Attestation {
    pub(crate) subject: String,
    pub(crate) repo: String,
    pub(crate) signer: String,
    pub(crate) git_ref: String,
    pub(crate) predicate: String,
}

impl Attestation {
    pub(crate) fn signed(key: &str) -> Self {
        Self::bound(key, ENTRY_ID, ENTRY_SIZE)
    }

    pub(crate) fn bound(key: &str, id: u64, size: u64) -> Self {
        Self::bound_text(key, &id.to_string(), &size.to_string())
    }

    pub(crate) fn bound_text(key: &str, id: &str, size: &str) -> Self {
        Self {
            subject: format!("{key} {id} {size}\n"),
            repo: REPO.to_owned(),
            signer: SIGNER.to_owned(),
            git_ref: MAIN.to_owned(),
            predicate: STATE_SEAL_PREDICATE_TYPE.to_owned(),
        }
    }

    fn row(&self) -> String {
        let digest = S256::of(self.subject.as_bytes()).to_string();
        format!(
            "{} {} {} {} {}\n",
            digest.trim_start_matches("s256-"),
            self.repo,
            self.signer,
            self.git_ref,
            self.predicate
        )
    }
}

impl Gh {
    pub(crate) fn install(scene: &Scene) -> Result<Self, Box<dyn Error>> {
        let dir = scene.scratch.join("fake-gh");
        std::fs::create_dir_all(&dir)?;
        scene.install_shim("gh", GH)?;
        let gh = Self { dir };
        gh.mode("ok")?;
        gh.list(&[])?;
        gh.attest(&[])?;
        gh.usage(0)?;
        gh.artifacts(&[])?;
        gh.refuse_delete(&[])?;
        gh.clear_log()?;
        Ok(gh)
    }

    pub(crate) fn tip(&self, sha: &str) -> Result<(), Box<dyn Error>> {
        std::fs::write(self.dir.join("tip"), format!("{sha}\n"))?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn fail_keys(&self, keys: &[&str]) -> Result<(), Box<dyn Error>> {
        std::fs::write(self.dir.join("fail-keys"), format!("{}\n", keys.join("\n")))?;
        Ok(())
    }

    pub(crate) fn lag(&self, empty_listings: u32) -> Result<(), Box<dyn Error>> {
        std::fs::write(self.dir.join("lag"), format!("{empty_listings}\n"))?;
        Ok(())
    }

    pub(crate) fn sleeps(&self) -> usize {
        std::fs::read_to_string(self.dir.join("sleeps")).map_or(0, |text| text.lines().count())
    }

    pub(crate) fn peak(&self) -> Result<usize, Box<dyn Error>> {
        let mut active = 0usize;
        let mut peak = 0usize;
        for event in std::fs::read_to_string(self.dir.join("activity"))?.lines() {
            match event {
                "+" => active += 1,
                "-" => active = active.saturating_sub(1),
                _ => return Err(fail("invalid fake GH activity event")),
            }
            peak = peak.max(active);
        }
        Ok(peak)
    }

    pub(crate) fn hung_pids(&self) -> Result<Vec<u32>, Box<dyn Error>> {
        Ok(std::fs::read_to_string(self.dir.join("hung-pids"))?
            .lines()
            .map(str::parse)
            .collect::<Result<_, _>>()?)
    }

    pub(crate) fn dir(&self) -> String {
        self.dir.display().to_string()
    }

    pub(crate) fn mode(&self, mode: &str) -> Result<(), Box<dyn Error>> {
        std::fs::write(self.dir.join("mode"), format!("{mode}\n"))?;
        Ok(())
    }

    pub(crate) fn list(&self, rows: &[(&str, &str, &str)]) -> Result<(), Box<dyn Error>> {
        let entries: Vec<(&str, &str, &str, u64, u64)> = rows
            .iter()
            .map(|(created, git_ref, key)| (*created, *git_ref, *key, ENTRY_ID, ENTRY_SIZE))
            .collect();
        self.list_entries(&entries)
    }

    pub(crate) fn list_entries(
        &self,
        rows: &[(&str, &str, &str, u64, u64)],
    ) -> Result<(), Box<dyn Error>> {
        let text = rows
            .iter()
            .map(|(created, git_ref, key, id, size)| {
                format!("{created}\t{git_ref}\t{key}\t{id}\t{size}\n")
            })
            .collect::<Vec<String>>()
            .concat();
        std::fs::write(self.dir.join("list"), text)?;
        Ok(())
    }

    pub(crate) fn rows(&self) -> Result<Vec<(String, u64)>, Box<dyn Error>> {
        let text = std::fs::read_to_string(self.dir.join("list"))?;
        text.lines()
            .map(|line| {
                let mut fields = line.split('\t');
                let key = fields.nth(2).ok_or_else(|| fail("list row lost its key"))?;
                let id = fields.next().ok_or_else(|| fail("list row lost its id"))?;
                Ok((key.to_owned(), id.parse()?))
            })
            .collect()
    }

    pub(crate) fn usage(&self, bytes: u64) -> Result<(), Box<dyn Error>> {
        std::fs::write(self.dir.join("usage"), format!("{bytes}\n"))?;
        Ok(())
    }

    pub(crate) fn artifacts(&self, ids: &[u64]) -> Result<(), Box<dyn Error>> {
        let text = ids.iter().fold(String::new(), |mut text, id| {
            text.push_str(&id.to_string());
            text.push('\n');
            text
        });
        std::fs::write(self.dir.join("artifacts"), text)?;
        Ok(())
    }

    pub(crate) fn refuse_delete(&self, ids: &[u64]) -> Result<(), Box<dyn Error>> {
        let text = ids.iter().fold(String::new(), |mut text, id| {
            text.push_str(&id.to_string());
            text.push('\n');
            text
        });
        std::fs::write(self.dir.join("refuse"), text)?;
        Ok(())
    }

    pub(crate) fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("log")).unwrap_or_default()
    }

    pub(crate) fn clear_log(&self) -> Result<(), Box<dyn Error>> {
        std::fs::write(self.dir.join("log"), "")?;
        Ok(())
    }

    pub(crate) fn list_accessed(
        &self,
        rows: &[(&str, &str, &str, &str)],
    ) -> Result<(), Box<dyn Error>> {
        let text = rows
            .iter()
            .map(|(created, git_ref, key, accessed)| {
                format!("{created}\t{git_ref}\t{key}\t{ENTRY_ID}\t{ENTRY_SIZE}\t{accessed}\n")
            })
            .collect::<Vec<String>>()
            .concat();
        std::fs::write(self.dir.join("list"), text)?;
        Ok(())
    }

    pub(crate) fn attest(&self, rows: &[Attestation]) -> Result<(), Box<dyn Error>> {
        let text: String = rows.iter().map(Attestation::row).collect();
        std::fs::write(self.dir.join("attest"), text)?;
        Ok(())
    }
}

pub(crate) fn digest(fill: char) -> String {
    format!("s256-{}", fill.to_string().repeat(64))
}

pub(crate) fn scope_of(root: &str) -> Result<StateScope, Box<dyn Error>> {
    Ok(StateScope {
        trust: StateTrust::Trusted,
        unit: StateUnit::Union,
        root: RootSlug::from_root(root)?,
    })
}

pub(crate) fn scope() -> Result<StateScope, Box<dyn Error>> {
    scope_of(".")
}

pub(crate) fn key(compat: char, snapshot: char, payload: char) -> Result<String, Box<dyn Error>> {
    Ok(format!(
        "{}{}-{}-{}",
        scope()?.prefix(),
        digest(compat),
        digest(snapshot),
        digest(payload)
    ))
}

pub(crate) fn script_of(step: &Step) -> Result<String, Box<dyn Error>> {
    match &step.kind {
        StepKind::Shell { run, .. } => run
            .get(3)
            .cloned()
            .ok_or_else(|| fail("shell step lost its script")),
        _ => Err(fail("expected a shell step")),
    }
}

pub(crate) fn select_script(root: &str) -> Result<String, Box<dyn Error>> {
    let root = SealRoot {
        unit: StateUnit::Union,
        slug: RootSlug::from_root(root)?,
    };
    script_of(&state_select_step(&[root], JobTimeout::PLAN)?)
}

pub(crate) fn run_select(
    scene: &Scene,
    gh: &Gh,
    root: &str,
    extra: &[(&str, &str)],
) -> Result<String, Box<dyn Error>> {
    match std::fs::remove_file(&scene.github_output) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let dir = gh.dir();
    let mut env = vec![
        ("FAKE_GH_DIR", dir.as_str()),
        ("GITHUB_REPOSITORY", REPO),
        ("GITHUB_WORKFLOW_REF", OTHER_WORKFLOW_REF),
        ("GH_TOKEN", "fixture"),
    ];
    env.extend_from_slice(extra);
    scene.run_script(&select_script(root)?, &env, None)
}

pub(crate) fn selected(scene: &Scene, root: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let slug = RootSlug::from_root(root)?;
    scene.output_block(&state_attested_output(
        StateUnit::Union.as_str(),
        slug.as_str(),
    ))
}
