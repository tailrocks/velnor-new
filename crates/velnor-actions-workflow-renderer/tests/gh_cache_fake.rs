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
  awk -F'\t' -v p="$key" -v r="$ref" 'index($2,p)==1 && (r=="" || $3==r) { print $4 "\t" $1 "\t" $2 "\t" 100 }' "$GH_FIXTURE" | sort -t "$tab" $flag | head -n "$limit" | cut -f2,3,4
  ;;
"cache delete")
  echo "$3" >> "$GH_DELETED"
  case " $GH_FAIL_DELETE " in *" $3 "*) exit 1;; esac
  ;;
"attestation verify")
  file=$3
  shift 3
  repo=; signer=; sref=; pred=
  while [ $# -gt 0 ]; do
    case "$1" in -R) repo=$2;; --signer-workflow) signer=$2;; --source-ref) sref=$2;; --predicate-type) pred=$2;; esac
    shift 2
  done
  [ "$repo" = "$GITHUB_REPOSITORY" ] && [ "$signer" = "$GITHUB_REPOSITORY/.github/workflows/state-seal.yml" ] && [ "$pred" = "https://velnor.dev/attestation/state-seal/v1" ] || exit 1
  read -r skey sid ssize < "$file"
  entry_ref=$(awk -F'\t' -v id="$sid" '$1==id { print $3 }' "$GH_FIXTURE")
  [ "$sref" = "$entry_ref" ] || exit 1
  case " $GH_ATTESTED " in *" $sid "*) exit 0;; esac
  exit 1
  ;;
*) exit 99;;
esac
"#;

pub(crate) struct Ledger {
    rows: Vec<(u32, String, String)>,
    attested: Vec<u32>,
}

impl Ledger {
    pub(crate) fn new() -> Self {
        Self {
            rows: Vec::new(),
            attested: Vec::new(),
        }
    }

    pub(crate) fn attest(&mut self, id: u32) -> u32 {
        self.attested.push(id);
        id
    }

    pub(crate) fn add(&mut self, id: u32, git_ref: &str, key: &str) -> u32 {
        self.rows.push((id, git_ref.to_owned(), key.to_owned()));
        id
    }
}

pub(crate) struct PruneRun {
    pub(crate) status: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) attempted: Vec<u32>,
}

pub(crate) struct GhFake {
    dir: TempDir,
    fail_list: bool,
    fail_delete: Vec<u32>,
    attested: Vec<u32>,
    locale: Option<&'static str>,
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
            attested: ledger.attested.clone(),
            locale: None,
        })
    }

    pub(crate) fn in_locale(mut self, locale: &'static str) -> Self {
        self.locale = Some(locale);
        self
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
        let head_branch = git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref);
        self.run_prune_for(head_branch)
    }

    pub(crate) fn run_prune_for(&self, head_branch: &str) -> Result<PruneRun, Box<dyn Error>> {
        let root: &Path = self.dir.path();
        let script = prune_script();
        let bootstrap = velnor_actions_workflow_renderer::verified_gh_shell::bootstrap();
        let command = velnor_actions_workflow_renderer::verified_gh_shell::command();
        assert!(script.contains(&bootstrap));
        assert!(script.contains(&command));
        let script = script
            .replace(&bootstrap, ":")
            .replace(&command, "command gh");
        let mut command = Command::new("sh");
        if let Some(locale) = self.locale {
            command.env("LC_ALL", locale);
        }
        let output = command
            .arg("-c")
            .arg(script)
            .env("PATH", format!("{}:/usr/bin:/bin", root.display()))
            .env("GITHUB_REF", "refs/heads/main")
            .env("HEAD_BRANCH", head_branch)
            .env("RUNNER_TEMP", root)
            .env("GH_FIXTURE", root.join("fixture"))
            .env("GH_DELETED", root.join("deleted"))
            .env("GITHUB_REPOSITORY", "acme/app")
            .env(
                "GITHUB_WORKFLOW_REF",
                "acme/app/.github/workflows/cache-prune.yml@refs/heads/main",
            )
            .env(
                "GH_ATTESTED",
                self.attested
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            )
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
            stderr: String::from_utf8(output.stderr)?,
            attempted,
        })
    }
}
