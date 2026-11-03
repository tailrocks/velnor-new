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
    key=; ref=; prog=.
    while [ $# -gt 0 ]; do
      case "$1" in --key) key=$2; shift ;; --ref) ref=$2; shift ;; --jq) prog=$2; shift ;; esac
      shift
    done
    if [ "$mode" = garbage ]; then printf '%s\n' '<<<not json>>>' '{"oops"'; exit 0; fi
    : > "$d/json"
    while read -r created lref lkey lid lsize; do
      [ "$lref" = "$ref" ] || continue
      case "$lkey" in "$key"*) printf '{"createdAt":"%s","key":"%s","id":%s,"sizeInBytes":%s}\n' "$created" "$lkey" "$lid" "$lsize" >> "$d/json" ;; esac
    done < "$d/list"
    jq -s -r "$prog" "$d/json"
    ;;
  "cache delete")
    id=$1
    if [ -f "$d/refuse" ] && grep -qx "$id" "$d/refuse"; then exit 1; fi
    grep -v "^[^ ]* [^ ]* [^ ]* $id " "$d/list" > "$d/list.next" || :
    mv "$d/list.next" "$d/list"
    printf 'delete cache %s\n' "$id" >> "$d/log"
    ;;
  "api actions/cache/usage"|"api repos/"*"/actions/cache/usage")
    [ "$mode" = usage_fail ] && exit 1
    prog=.
    while [ $# -gt 0 ]; do
      case "$1" in --jq) prog=$2; shift ;; esac
      shift
    done
    read -r usage < "$d/usage"
    printf '{"active_caches_size_in_bytes":%s}\n' "$usage" | jq -r "$prog"
    ;;
  "api -X")
    [ "$1" = DELETE ] || exit 2
    path=$2
    id=${path##*/}
    grep -qx "$id" "$d/artifacts" || exit 1
    grep -vx "$id" "$d/artifacts" > "$d/artifacts.next" || :
    mv "$d/artifacts.next" "$d/artifacts"
    printf 'delete artifact %s\n' "$id" >> "$d/log"
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

pub(crate) struct Entry<'a> {
    pub(crate) created: &'a str,
    pub(crate) git_ref: &'a str,
    pub(crate) key: &'a str,
    pub(crate) id: u64,
    pub(crate) size: u64,
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
        gh.usage(0)?;
        gh.artifacts(&[])?;
        gh.refuse_delete(&[])?;
        gh.clear_log()?;
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
        let entries: Vec<Entry<'_>> = rows
            .iter()
            .enumerate()
            .map(|(index, (created, git_ref, key))| Entry {
                created,
                git_ref,
                key,
                id: 100 + index as u64,
                size: 1,
            })
            .collect();
        self.entries(&entries)
    }

    pub(crate) fn entries(&self, rows: &[Entry<'_>]) -> Result<(), Box<dyn Error>> {
        let text: String = rows
            .iter()
            .map(|row| {
                format!(
                    "{} {} {} {} {}\n",
                    row.created, row.git_ref, row.key, row.id, row.size
                )
            })
            .collect();
        std::fs::write(self.dir.join("list"), text)?;
        Ok(())
    }

    pub(crate) fn rows(&self) -> Result<Vec<(String, u64)>, Box<dyn Error>> {
        let text = std::fs::read_to_string(self.dir.join("list"))?;
        text.lines()
            .map(|line| {
                let mut fields = line.split(' ');
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
        let text: String = ids.iter().map(|id| format!("{id}\n")).collect();
        std::fs::write(self.dir.join("artifacts"), text)?;
        Ok(())
    }

    pub(crate) fn refuse_delete(&self, ids: &[u64]) -> Result<(), Box<dyn Error>> {
        let text: String = ids.iter().map(|id| format!("{id}\n")).collect();
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

pub(crate) fn select_script(root: &str) -> Result<String, Box<dyn Error>> {
    let root = SealRoot {
        unit: StateUnit::Union,
        slug: RootSlug::from_root(root)?,
    };
    script_of(&state_select_step(&[root])?)
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
        (
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main",
        ),
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
