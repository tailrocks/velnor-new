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
use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealRoot, state_select_step};

use crate::gc1_scene::{Scene, fail};

pub(crate) const REPO: &str = "tailrocks/velnor-new";
pub(crate) const SIGNER: &str = "tailrocks/velnor-new/.github/workflows/ci.yml";
pub(crate) const MAIN: &str = "refs/heads/main";
pub(crate) const PULL: &str = "refs/pull/9/merge";

const GH: &str = r#"#!/bin/sh
d="${FAKE_GH_DIR:?}"
mode=ok
[ -f "$d/mode" ] && read -r mode < "$d/mode"
case "$mode" in
  fail) exit 1 ;;
  hang) sleep 30; exit 0 ;;
esac
verb="$1 $2"
shift 2
case "$verb" in
  "cache list")
    key=; ref=
    while [ $# -gt 0 ]; do
      case "$1" in --key) key=$2; shift ;; --ref) ref=$2; shift ;; esac
      shift
    done
    if [ "$mode" = garbage ]; then printf '%s\n' '<<<not json>>>' '{"oops"'; exit 0; fi
    while read -r created lref lkey; do
      [ "$lref" = "$ref" ] || continue
      case "$lkey" in "$key"*) printf '%s %s\n' "$created" "$lkey" ;; esac
    done < "$d/list"
    ;;
  "attestation verify")
    file=$1
    shift
    repo=; signer=; sref=; pred=
    while [ $# -gt 0 ]; do
      case "$1" in -R) repo=$2; shift ;; --signer-workflow) signer=$2; shift ;; --source-ref) sref=$2; shift ;; --predicate-type) pred=$2; shift ;; esac
      shift
    done
    [ -n "$repo" ] || exit 1
    sum=$(sha256sum "$file")
    sum=${sum%% *}
    while read -r tdigest trepo tsigner tref tpred; do
      [ "$tdigest" = "$sum" ] || continue
      [ "$repo" = "$trepo" ] || continue
      [ -z "$signer" ] || [ "$signer" = "$tsigner" ] || continue
      [ -z "$sref" ] || [ "$sref" = "$tref" ] || continue
      [ -z "$pred" ] || [ "$pred" = "$tpred" ] || continue
      exit 0
    done < "$d/attest"
    exit 1
    ;;
  *) exit 2 ;;
esac
"#;

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
        Self {
            subject: format!("{key}\n"),
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
        Ok(gh)
    }

    pub(crate) fn dir(&self) -> String {
        self.dir.display().to_string()
    }

    pub(crate) fn mode(&self, mode: &str) -> Result<(), Box<dyn Error>> {
        std::fs::write(self.dir.join("mode"), format!("{mode}\n"))?;
        Ok(())
    }

    pub(crate) fn list(&self, rows: &[(&str, &str, &str)]) -> Result<(), Box<dyn Error>> {
        let text = rows
            .iter()
            .fold(String::new(), |mut text, (created, git_ref, key)| {
                text.push_str(created);
                text.push(' ');
                text.push_str(git_ref);
                text.push(' ');
                text.push_str(key);
                text.push('\n');
                text
            });
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
            .get(2)
            .cloned()
            .ok_or_else(|| fail("shell step lost its script")),
        _ => Err(fail("expected a shell step")),
    }
}

pub(crate) fn select_script(root: &str, canary: bool) -> Result<String, Box<dyn Error>> {
    let root = SealRoot {
        unit: StateUnit::Union,
        slug: RootSlug::from_root(root)?,
    };
    script_of(&state_select_step(&[root], canary)?)
}

pub(crate) fn run_select(
    scene: &Scene,
    gh: &Gh,
    root: &str,
    canary: bool,
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
        (
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main",
        ),
        ("GH_TOKEN", "fixture"),
    ];
    env.extend_from_slice(extra);
    scene.run_script(&select_script(root, canary)?, &env, None)
}

pub(crate) fn selected(scene: &Scene, root: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let slug = RootSlug::from_root(root)?;
    scene.output_block(&state_attested_output(
        StateUnit::Union.as_str(),
        slug.as_str(),
    ))
}
