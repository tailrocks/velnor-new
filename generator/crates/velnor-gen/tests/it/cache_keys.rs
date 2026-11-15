// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CacheKey, CacheRoot, CiDebugInfo, Concurrency, Host, Job, JobId, Permissions, PullRequestType,
    RepoPath, RootSlug, RunnerLabel, Step, StepKind, Triggers, WORKFLOW_PATH, When, WorkflowPlan,
};
use velnor_render::{RenderedKey, render, render_key};
use velnor_rust::{
    CONFIG_PATH, CompatInputs, DetectError, Reason, build_profile, ci_debuginfo, compat, detect,
};

use crate::facts::{TestResult, lib, repo};
use crate::pipeline::{view, workflow};

const HOST: &str = "${{ runner.os }}-${{ runner.arch }}";
const COMPAT: &str = "${{ steps.prepare.outputs.compat }}";

fn root(manifest: &str) -> Result<CacheRoot, Box<dyn std::error::Error>> {
    Ok(CacheRoot::new(&RepoPath::new(manifest)?))
}

fn target(job: &str, manifest: &str) -> Result<RenderedKey, Box<dyn std::error::Error>> {
    Ok(render_key(&CacheKey::Target {
        job: JobId::new(job)?,
        root: root(manifest)?,
        host: Host,
    }))
}

#[test]
fn each_family_renders_its_key_and_ordered_fallbacks() -> TestResult {
    let tools = render_key(&CacheKey::Tools { host: Host });
    assert_eq!(
        tools,
        RenderedKey {
            key: format!("velnor-tools-{HOST}-${{{{ hashFiles('mise.lock') }}}}"),
            restore_keys: vec![],
        }
    );
    let sources = render_key(&CacheKey::Sources {
        host: Host,
        root: root("lab/Cargo.toml")?,
    });
    assert_eq!(
        sources,
        RenderedKey {
            key: format!("velnor-src-{HOST}-lab-${{{{ hashFiles('lab/Cargo.lock') }}}}"),
            restore_keys: vec![format!("velnor-src-{HOST}-lab-")],
        }
    );
    let lock = "${{ hashFiles('Cargo.lock') }}";
    assert_eq!(
        target("rust-lint", "Cargo.toml")?,
        RenderedKey {
            key: format!(
                "velnor-tgt-rust-lint-root-{HOST}-{COMPAT}-{lock}-${{{{ github.sha }}}}-${{{{ github.run_id }}}}"
            ),
            restore_keys: vec![
                format!("velnor-tgt-rust-lint-root-{HOST}-{COMPAT}-{lock}-"),
                format!("velnor-tgt-rust-lint-root-{HOST}-{COMPAT}-"),
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
                "velnor-dlt-rust-test-root-{HOST}-{COMPAT}-${{{{ steps.restored.outputs.base8 }}}}-${{{{ github.run_id }}}}"
            ),
            restore_keys: vec![format!(
                "velnor-dlt-rust-test-root-{HOST}-{COMPAT}-${{{{ steps.restored.outputs.base8 }}}}-"
            )],
        }
    );
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
    let sources = render_key(&CacheKey::Sources {
        host: Host,
        root: root("a'b/Cargo.toml")?,
    });
    assert!(
        sources
            .key
            .ends_with("-${{ hashFiles('a''b/Cargo.lock') }}"),
        "{}",
        sources.key
    );
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
        root: root("Cargo.toml")?,
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
        jobs: vec![Job {
            id: JobId::new("rust")?,
            name: None,
            comment: None,
            needs: Vec::new(),
            when: When::Success,
            skip_on_draft: false,
            runs_on: RunnerLabel::new("ubuntu-latest")?,
            permissions: Permissions::default(),
            timeout_minutes: 10,
            gate: None,
            plan_outputs: Vec::new(),
            steps: vec![Step {
                kind: StepKind::CacheSave(key.clone()),
                directory: None,
                when: When::Success,
                skip_on_draft: false,
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

type Mutation = (&'static str, fn(&mut CompatInputs));

fn inputs() -> CompatInputs {
    CompatInputs {
        rustc_verbose: "rustc 1.99.1\nhost: aarch64-apple-darwin\n".to_owned(),
        target: None,
        profile: build_profile(CiDebugInfo::LineTablesOnly),
        rustflags: String::new(),
        workspace: "/home/runner/work/p/p".to_owned(),
        driver: "cargo".to_owned(),
    }
}

#[test]
fn changing_any_compat_input_changes_the_digest() {
    let base = compat(&inputs());
    assert_eq!(base, compat(&inputs()));
    assert_eq!(base.len(), 16);
    let mutations: [Mutation; 9] = [
        ("rustc", |i| i.rustc_verbose.push_str("release: 1.99.2\n")),
        ("target", |i| {
            i.target = Some("x86_64-unknown-linux-gnu".to_owned());
        }),
        ("debuginfo full", |i| {
            i.profile = build_profile(CiDebugInfo::Full);
        }),
        ("debuginfo none", |i| {
            i.profile = build_profile(CiDebugInfo::None);
        }),
        ("rustflags", |i| {
            i.rustflags = "-C target-cpu=native".to_owned();
        }),
        ("workspace", |i| i.workspace = "/work".to_owned()),
        ("driver", |i| i.driver = "mbx".to_owned()),
        ("env", |i| i.profile.env.push(("EXTRA", "1"))),
        ("config", |i| i.profile.cargo_config = "[build]\n"),
    ];
    for (name, mutate) in mutations {
        let mut changed = inputs();
        mutate(&mut changed);
        assert_ne!(base, compat(&changed), "{name}");
    }
    let (mut left, mut right) = (inputs(), inputs());
    left.rustflags = "a".to_owned();
    left.workspace = "b".to_owned();
    right.rustflags = "ab".to_owned();
    right.workspace = String::new();
    assert_ne!(compat(&left), compat(&right));
    let (mut left, mut right) = (inputs(), inputs());
    left.rustflags = "x\nworkspace:y".to_owned();
    left.workspace = "z".to_owned();
    right.rustflags = "x".to_owned();
    right.workspace = "y\nworkspace:z".to_owned();
    assert_ne!(compat(&left), compat(&right));
}

#[test]
fn the_ci_debuginfo_key_picks_the_profile_and_rejects_other_values() -> TestResult {
    let files =
        |text: &str| std::collections::BTreeMap::from([(CONFIG_PATH, text.as_bytes().to_vec())]);
    assert_eq!(
        ci_debuginfo(&std::collections::BTreeMap::new()),
        Ok(CiDebugInfo::LineTablesOnly)
    );
    for (value, expected, workspace) in [
        ("full", CiDebugInfo::Full, "full"),
        (
            "line-tables-only",
            CiDebugInfo::LineTablesOnly,
            "line-tables-only",
        ),
        ("none", CiDebugInfo::None, "none"),
    ] {
        let found = ci_debuginfo(&files(&format!("ci_debuginfo = \"{value}\"\n")))?;
        assert_eq!(found, expected);
        let profile = build_profile(found);
        assert!(
            profile
                .env
                .contains(&("CARGO_PROFILE_DEV_DEBUG", workspace))
        );
        assert!(
            profile
                .env
                .contains(&("CARGO_PROFILE_TEST_DEBUG", workspace))
        );
        assert_eq!(
            profile.cargo_config.is_empty(),
            expected == CiDebugInfo::Full
        );
    }
    assert_eq!(
        ci_debuginfo(&files("ci_debuginfo = \"some\"\n")),
        Err(DetectError::Invalid {
            path: CONFIG_PATH,
            reason: Reason::InvalidValue {
                key: "ci_debuginfo",
                value: "some".to_owned(),
            },
        })
    );
    let mut view = repo(vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])], &[])?;
    view.tracked_paths.insert(CONFIG_PATH);
    view.files
        .insert(CONFIG_PATH, b"ci_debuginfo = \"none\"\n".to_vec());
    assert_eq!(detect(&view)?.config.ci_debuginfo, CiDebugInfo::None);
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
    assert!(text.contains("velnor-tgt-rust-lint-root-root-"));
    assert!(text.contains("velnor-tgt-rust-lint-lab-lab-"));
    assert!(text.contains("path: lab/target\n"));
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
