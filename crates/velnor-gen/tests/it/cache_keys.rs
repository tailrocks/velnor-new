// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CacheKey, CacheRoot, Concurrency, Host, Job, JobId, Permissions, PrRun, PullRequestType,
    RepoPath, RootSlug, RunnerLabel, Step, StepKind, Triggers, WORKFLOW_PATH, When, WorkflowPlan,
};
use velnor_render::{RenderedKey, render, render_key, render_key_with_mise};

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const HOST: &str = "${{ runner.os }}-${{ runner.arch }}";
const COMPAT: &str = "${{ steps.prepare.outputs.compat }}";

fn root(manifest: &str) -> Result<CacheRoot, Box<dyn std::error::Error>> {
    Ok(CacheRoot::new(
        &RepoPath::new(manifest)?,
        RepoPath::new("target")?,
    ))
}

fn target(job: &str, manifest: &str) -> Result<RenderedKey, Box<dyn std::error::Error>> {
    Ok(render_key(&CacheKey::Target {
        job: JobId::new(job)?,
        root: root(manifest)?,
        host: Host,
    }))
}

#[test]
fn a_mise_bump_changes_the_tools_key() {
    let key = CacheKey::Tools { host: Host };
    let old = render_key_with_mise(&key, "2026.11.1");
    let new = render_key_with_mise(&key, "2026.12.0");
    assert_ne!(old.key, new.key);
    assert!(new.key.contains("mise2026.12.0"), "{}", new.key);
}

#[test]
fn each_family_renders_its_key_and_ordered_fallbacks() -> TestResult {
    let tools = render_key(&CacheKey::Tools { host: Host });
    assert_eq!(
        tools,
        RenderedKey {
            key: format!("velnor-tools-{HOST}-mise2026.11.10-${{{{ hashFiles('mise.lock') }}}}"),
            restore_keys: vec![],
        }
    );
    let sources = render_key(&CacheKey::Sources {
        host: Host,
        lane: root("lab/Cargo.toml")?,
    });
    assert_eq!(
        sources,
        RenderedKey {
            key: format!("velnor-src-{HOST}-all.roots-${{{{ steps.sources_locks.outputs.hash }}}}"),
            restore_keys: vec![format!("velnor-src-{HOST}-all.roots-")],
        }
    );
    let lock = "${{ hashFiles('Cargo.lock') }}";
    assert_eq!(
        target("rust-lint", "Cargo.toml")?,
        RenderedKey {
            key: format!(
                "velnor-tgt-rust-lint.root.{HOST}-{COMPAT}-{lock}-${{{{ github.sha }}}}-${{{{ github.run_id }}}}"
            ),
            restore_keys: vec![
                format!("velnor-tgt-rust-lint.root.{HOST}-{COMPAT}-{lock}-"),
                format!("velnor-tgt-rust-lint.root.{HOST}-{COMPAT}-"),
            ],
        }
    );
    assert_eq!(
        render_key(&CacheKey::Delta {
            job: JobId::new("rust-test")?,
            root: root("Cargo.toml")?,
            host: Host,
        }),
        RenderedKey {
            key: format!(
                "velnor-dlt-pr${{{{ github.event.pull_request.number }}}}-rust-test.root.{HOST}-{COMPAT}-${{{{ steps.delta_base_root.outputs.base8 }}}}-${{{{ github.run_id }}}}"
            ),
            restore_keys: vec![format!(
                "velnor-dlt-pr${{{{ github.event.pull_request.number }}}}-rust-test.root.{HOST}-{COMPAT}-${{{{ steps.delta_base_root.outputs.base8 }}}}-"
            )],
        }
    );
    Ok(())
}

#[test]
fn a_delta_key_is_never_equal_to_or_a_prefix_of_a_base_key() -> TestResult {
    let mut keys = Vec::new();
    for (manifest, target) in [("Cargo.toml", "target"), ("lab/Cargo.toml", "lab/target")] {
        let root = CacheRoot::new(&RepoPath::new(manifest)?, RepoPath::new(target)?);
        for job in ["rust", "rust-test", "dlt"] {
            let job = JobId::new(job)?;
            keys.push(CacheKey::Target {
                job: job.clone(),
                root: root.clone(),
                host: Host,
            });
            keys.push(CacheKey::Delta {
                job,
                root: root.clone(),
                host: Host,
            });
        }
        keys.push(CacheKey::Sources {
            host: Host,
            lane: root,
        });
    }
    keys.push(CacheKey::Tools { host: Host });
    let mut tags = std::collections::BTreeSet::new();
    let mut deltas = Vec::new();
    let mut bases = Vec::new();
    for key in &keys {
        tags.insert(key.tag());
        let rendered = render_key(key);
        let family = format!("velnor-{}-", key.tag());
        for value in std::iter::once(rendered.key).chain(rendered.restore_keys) {
            assert!(value.starts_with(&family), "{value}");
            if matches!(key, CacheKey::Delta { .. }) {
                deltas.push(value);
            } else {
                bases.push(value);
            }
        }
    }
    assert_eq!(tags.len(), 4, "{tags:?}");
    assert!(!deltas.is_empty() && !bases.is_empty());
    for delta in &deltas {
        for base in &bases {
            assert_ne!(delta, base);
            assert!(!delta.starts_with(base.as_str()), "{base} prefixes {delta}");
            assert!(!base.starts_with(delta.as_str()), "{delta} prefixes {base}");
        }
    }
    Ok(())
}

#[test]
fn a_key_names_its_job_and_root_and_never_a_branch_or_a_digest() -> TestResult {
    let jobs = ["rust", "rust-lint", "rust-test"];
    let roots = ["Cargo.toml", "lab/Cargo.toml", "tools/xtask/Cargo.toml"];
    let mut keys = Vec::new();
    for job in jobs {
        for manifest in roots {
            keys.push(target(job, manifest)?);
        }
    }
    for rendered in &keys {
        let lines = std::iter::once(&rendered.key).chain(&rendered.restore_keys);
        for line in lines {
            assert!(!line.contains("ref"), "{line}");
            assert!(!line.contains("branch"), "{line}");
            let literal: String = line
                .split("${{")
                .map(|part| part.split_once("}}").map_or(part, |(_, rest)| rest))
                .collect();
            assert!(
                !literal
                    .split(|c: char| !c.is_ascii_hexdigit())
                    .any(|run| run.len() >= 32),
                "{line}"
            );
        }
        let first = &rendered.restore_keys[0];
        let second = &rendered.restore_keys[1];
        assert!(rendered.key.starts_with(first) && rendered.key.len() > first.len());
        assert!(first.starts_with(second) && first.len() > second.len());
    }
    let distinct: std::collections::BTreeSet<&String> = keys.iter().map(|k| &k.key).collect();
    assert_eq!(distinct.len(), keys.len());
    Ok(())
}

#[test]
fn a_directory_name_cannot_close_or_open_an_expression_in_a_key() -> TestResult {
    let key = target("rust", "a${{ github.event.title }}'/Cargo.toml")?;
    for line in std::iter::once(&key.key).chain(&key.restore_keys) {
        assert_eq!(
            line.matches("${{").count(),
            line.matches("}}").count(),
            "{line}"
        );
    }
    assert!(key.key.contains("hashFiles(format('{0}{1}"), "{}", key.key);
    Ok(())
}

#[test]
fn a_quote_in_a_directory_name_is_doubled_inside_the_hash_files_literal() -> TestResult {
    let hash = crate::shared_sources::locks_hash(&["a'b/Cargo.toml"])?;
    assert_eq!(hash, "${{ hashFiles('a''b/Cargo.lock') }}");
    Ok(())
}

#[test]
fn a_root_slug_never_holds_a_dash_so_a_job_and_a_root_cannot_blur() {
    for text in ["a-b", "-", "a-", "A", "a b", ""] {
        assert!(RootSlug::new(text).is_err(), "{text:?}");
    }
    assert!(RootSlug::new("a_b9").is_ok());
}

#[test]
fn a_save_step_renders_the_same_key_without_fallbacks() -> TestResult {
    let key = CacheKey::Sources {
        host: Host,
        lane: root("Cargo.toml")?,
    };
    let plan = WorkflowPlan {
        name: "Rust CI".to_owned(),
        triggers: Triggers {
            pull_request: vec![PullRequestType::Opened],
            push_branch: None,
            merge_group: false,
            schedule: None,
            workflow_dispatch: false,
        },
        concurrency: Concurrency::CancelOlderPullRequestRuns,
        permissions: Permissions::default(),
        fork_runner: RunnerLabel::new("ubuntu-latest")?,
        hosted_runners: velnor_model::HostedRunners::new(vec![RunnerLabel::new("ubuntu-latest")?]),
        trusted_runners: velnor_model::TrustedRunners::default(),
        jobs: vec![Job {
            id: JobId::new("rust")?,
            name: None,
            comment: None,
            needs: Vec::new(),
            when: When::Success,
            pr: PrRun::Always,
            runs_on: RunnerLabel::new("ubuntu-latest")?,
            permissions: Permissions::default(),
            timeout_minutes: 10,
            gate: None,
            skip_after_merge_group: None,
            plan_outputs: Vec::new(),
            group_outputs: Vec::new(),
            reports_ignored: false,
            records_build: false,
            chained_saves: false,
            selection_name: None,
            steps: vec![Step {
                kind: StepKind::CacheSave(key.clone()),
                directory: None,
                manifest: None,
                target_dir: None,
                when: When::Success,
                skip_after_merge_group: None,
                skip_when_cold: false,
                hosted_only: false,
                pr: PrRun::Always,
                gate: None,
                reads: None,
            }],
        }],
    };
    let files = render(&plan).map_err(|found| format!("{found:?}"))?;
    let text = files.get(&WORKFLOW_PATH).ok_or("no workflow")?;
    assert!(text.contains("        uses: actions/cache/save@"), "{text}");
    assert!(text.contains(&format!(
        "          key: '{}'\n",
        render_key(&key).key.replace('\'', "''")
    )));
    assert!(!text.contains("restore-keys"), "{text}");
    assert!(text.contains("path: |\n"), "{text}");
    assert!(text.contains("~/.cargo/registry/index\n"));
    Ok(())
}

fn lane_steps<'t>(text: &'t str, job: &str) -> Vec<&'t str> {
    let header = format!("  {job}:\n");
    let start = text
        .find(&header)
        .map_or(text.len(), |at| at + header.len());
    text[start..]
        .lines()
        .take_while(|line| {
            !(line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
        })
        .filter_map(|line| {
            line.strip_prefix("      - name: ")
                .or_else(|| line.strip_prefix("        name: "))
        })
        .collect()
}

#[test]
fn every_lane_restores_sources_then_its_own_target_between_prepare_and_restored() -> TestResult {
    let text = workflow(&view(
        &["Cargo.toml", "lab/Cargo.toml"],
        Some(b"shape = \"lint-test\"\n"),
    )?)?;
    for job in [
        "rust-lint-root",
        "rust-lint-lab",
        "rust-test-root",
        "rust-test-lab",
    ] {
        let steps = lane_steps(&text, job);
        let at = |name: &str| steps.iter().position(|step| *step == name);
        let (prepare, sources, target, restored) = (
            at("velnor-ci prepare"),
            at("Restore the sources cache"),
            at("Restore the target cache"),
            at("velnor-ci restored"),
        );
        assert!(
            prepare < sources && sources < target && target < restored,
            "{job}: {steps:?}"
        );
        assert!(restored.is_some_and(|restored| restored + 1 < steps.len()));
    }
    assert!(text.contains("velnor-tgt-rust-lint-root.root."));
    assert!(text.contains("velnor-tgt-rust-lint-lab.lab."));
    assert!(text.contains("lab/target"));
    assert!(!text.contains("target/.velnor/inputs.tsv"));
    assert!(
        !lane_steps(&text, "rust-plan")
            .iter()
            .any(|step| step.contains("cache"))
    );
    Ok(())
}

#[test]
fn a_dependency_bump_changes_no_byte_of_the_workflow() -> TestResult {
    let lock = RepoPath::new("Cargo.lock")?;
    let mut before = view(&["Cargo.toml"], None)?;
    before.files.insert(
        lock.clone(),
        b"[[package]]\nname = \"serde\"\nversion = \"1.0.1\"\n".to_vec(),
    );
    let mut after = before.clone();
    after.files.insert(
        lock,
        b"[[package]]\nname = \"serde\"\nversion = \"1.0.2\"\n".to_vec(),
    );
    assert_eq!(workflow(&before)?, workflow(&after)?);
    Ok(())
}
