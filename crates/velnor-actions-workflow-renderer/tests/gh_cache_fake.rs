// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;
use velnor_actions_workflow_renderer::cache_prune::prune_script;

const GH_SHIM: &str = r#"#!/bin/sh
tab=$(printf '\t')
case "$1 $2" in
"cache list")
  shift 2
  key=; ref=; limit=1000; order=desc
  while [ $# -gt 0 ]; do
    case "$1" in
      --key) key=$2; shift 2;;
      --ref) ref=$2; shift 2;;
      --limit) limit=$2; shift 2;;
      --order) order=$2; shift 2;;
      --sort) [ "$2" = created_at ] || exit 64; shift 2;;
      --json|--jq) shift 2;;
      *) exit 64;;
    esac
  done
  [ -z "$GH_FAIL_LIST" ] || exit 1
  flag=-k1,1nr
  [ "$order" = desc ] || flag=-k1,1n
  awk -F'\t' -v p="$key" -v r="$ref" 'index($2,p)==1 && (r=="" || $3==r) { print $4 "\t" $1 "\t" $2 }' "$GH_FIXTURE" | sort -t "$tab" $flag | head -n "$limit" | cut -f2,3
  ;;
"cache delete")
  echo "$3" >> "$GH_DELETED"
  case " $GH_FAIL_DELETE " in *" $3 "*) exit 1;; esac
  ;;
*) exit 99;;
esac
"#;

pub(crate) struct Ledger {
    rows: Vec<(u32, String, String)>,
}

impl Ledger {
    pub(crate) fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub(crate) fn add(&mut self, id: u32, git_ref: &str, key: &str) -> u32 {
        self.rows.push((id, git_ref.to_owned(), key.to_owned()));
        id
    }
}

pub(crate) struct PruneRun {
    pub(crate) status: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) attempted: Vec<u32>,
}

pub(crate) struct GhFake {
    dir: TempDir,
    fail_list: bool,
    fail_delete: Vec<u32>,
}

impl GhFake {
    pub(crate) fn new(ledger: &Ledger) -> Result<Self, Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let mut fixture = String::new();
        for (created, (id, git_ref, key)) in ledger.rows.iter().enumerate() {
            writeln!(fixture, "{id}\t{key}\t{git_ref}\t{created}")?;
        }
        fs::write(dir.path().join("fixture"), fixture)?;
        let shim = dir.path().join("gh");
        fs::write(&shim, GH_SHIM)?;
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755))?;
        Ok(Self {
            dir,
            fail_list: false,
            fail_delete: Vec::new(),
        })
    }

    pub(crate) fn failing_list(mut self) -> Self {
        self.fail_list = true;
        self
    }

    pub(crate) fn failing_delete(mut self, ids: &[u32]) -> Self {
        self.fail_delete = ids.to_vec();
        self
    }

    pub(crate) fn run_prune(&self, git_ref: &str) -> Result<PruneRun, Box<dyn Error>> {
        let root: &Path = self.dir.path();
        let output = Command::new("sh")
            .arg("-c")
            .arg(prune_script())
            .env("PATH", format!("{}:/usr/bin:/bin", root.display()))
            .env("GITHUB_REF", git_ref)
            .env("RUNNER_TEMP", root)
            .env("GH_FIXTURE", root.join("fixture"))
            .env("GH_DELETED", root.join("deleted"))
            .env("GH_FAIL_LIST", if self.fail_list { "1" } else { "" })
            .env(
                "GH_FAIL_DELETE",
                self.fail_delete
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            )
            .output()?;
        let attempted = fs::read_to_string(root.join("deleted"))
            .unwrap_or_default()
            .lines()
            .map(str::parse)
            .collect::<Result<Vec<u32>, _>>()?;
        Ok(PruneRun {
            status: output.status.code(),
            stdout: String::from_utf8(output.stdout)?,
            attempted,
        })
    }
}
