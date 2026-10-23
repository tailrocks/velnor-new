//! The tools-cache identity binds exactly the resolved versions and lock
//! entries of the project-declared tools each job installs (hard decision 5).
//!
//! The generated resolver scripts run under `bash` with the real `mise`
//! binary against a scratch checkout; every rendered `d` input is then
//! filled from the exported `env` values, as the identity step sees it.

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use velnor_actions_mise::RUST_VERSION;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::RenderedTree;
use velnor_actions_workflow_renderer::{VERIFY_PATH, toolchain_env::credential_scrub};

use crate::impl_common::{TestResult, make_repo};

const CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust]\ncompile_driver = \"mbx\"\ntest_runner = \"cargo_nextest\"\n";

fn mise_toml(nextest: &str, node: &str, task: &str) -> String {
    format!(
        "# project tools\n[tools]\nrust = \"{RUST_VERSION}\"\nmr-boxington = \"1.24.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"{nextest}\"\nnode = \"{node}\"\n\n[tasks.lint]\nrun = \"{task}\"\n"
    )
}

fn lock_entry(key: &str, version: &str, linux: &str, macos: &str) -> String {
    format!(
        "[[tools.{key}]]\nversion = \"{version}\"\nbackend = \"core:x\"\n\n[tools.{key}.\"platforms.linux-x64\"]\nchecksum = \"sha256:{linux}\"\nurl = \"https://example.invalid/{version}-linux\"\nurl_api = \"https://api.invalid/{linux}\"\n\n[tools.{key}.\"platforms.macos-arm64\"]\nchecksum = \"sha256:{macos}\"\nurl = \"https://example.invalid/{version}-macos\"\n\n"
    )
}

fn mise_lock(
    nextest: &str,
    nextest_sum: &str,
    rust_linux: &str,
    rust_macos: &str,
    node: &str,
) -> String {
    let mut lock = String::from("lockfile_version = 3\n\n");
    lock.push_str(&lock_entry(
        "\"aqua:nextest-rs/nextest/cargo-nextest\"",
        nextest,
        nextest_sum,
        "ff",
    ));
    lock.push_str(&lock_entry("mr-boxington", "1.24.1", "aa", "ab"));
    lock.push_str(&lock_entry("node", node, &format!("{node}0"), "cd"));
    lock.push_str(&lock_entry("rust", RUST_VERSION, rust_linux, rust_macos));
    lock
}

/// Decode a YAML double-quoted scalar the generator emits.
fn unquote(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn quoted_value(line: &str, prefix: &str) -> Option<String> {
    let raw = line.strip_prefix(prefix)?;
    let bytes = raw.as_bytes();
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'"' && !escaped {
            return Some(unquote(&raw[..index]));
        }
        if *byte == b'\\' {
            escaped = !escaped;
        } else {
            escaped = false;
        }
    }
    None
}

pub(super) struct ResolverStep {
    pub(super) shell: String,
    pub(super) run: String,
    pub(super) env: BTreeMap<String, String>,
}

pub(super) fn resolver_steps(yaml: &str) -> Vec<ResolverStep> {
    let lines = yaml.lines().collect::<Vec<_>>();
    let mut steps = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed != "- name: Resolve project tool versions" {
            continue;
        }
        let indent = line.len() - trimmed.len();
        let mut shell = None;
        let mut run = None;
        let mut env = BTreeMap::new();
        let mut in_env = false;
        for next in &lines[index + 1..] {
            let content = next.trim_start();
            let next_indent = next.len() - content.len();
            if !content.is_empty() && next_indent <= indent {
                break;
            }
            if content.is_empty() {
                continue;
            }
            if next_indent == indent + 2 {
                in_env = content == "env:";
                if let Some(value) = content.strip_prefix("shell: ") {
                    shell = Some(yaml_scalar(value));
                } else if let Some(value) = quoted_value(content, "run: \"") {
                    run = Some(value);
                }
            } else if in_env
                && next_indent == indent + 4
                && let Some((key, value)) = content.split_once(": ")
            {
                env.insert(key.to_owned(), yaml_scalar(value));
            }
        }
        if let (Some(shell), Some(run)) = (shell, run) {
            steps.push(ResolverStep { shell, run, env });
        }
    }
    steps
}

fn yaml_scalar(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(value) = quoted_value(raw, "\"") {
        return value;
    }
    raw.split(" # ").next().unwrap_or(raw).to_owned()
}

pub(super) fn rendered_tree() -> Result<RenderedTree, Box<dyn std::error::Error>> {
    let repo = make_repo(CONFIG)?;
    fs::write(
        repo.path().join("mise.toml"),
        mise_toml("0.9.148", "20.1.0", "a"),
    )?;
    Ok(render_staged_tree(&prepare(repo.path())?)?)
}

/// Every resolver script and every identity `d` input across the tree.
fn generated() -> Result<(Vec<String>, Vec<String>), Box<dyn std::error::Error>> {
    let tree = rendered_tree()?;
    let mut scripts = Vec::new();
    let mut inputs = Vec::new();
    for file in &tree.files {
        for line in file.bytes.lines().map(str::trim_start) {
            if let Some(value) = line.strip_prefix("d: ")
                && value
                    .get(..64)
                    .is_some_and(|digest| digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
            {
                inputs.push(value.to_owned());
            }
        }
        scripts.extend(resolver_steps(&file.bytes).into_iter().map(|step| step.run));
    }
    scripts.sort();
    scripts.dedup();
    inputs.sort();
    inputs.dedup();
    Ok((scripts, inputs))
}

/// Run every resolver over a checkout of `files` and fill each `d` input.
fn identities(
    generated: &(Vec<String>, Vec<String>),
    files: &[(&str, &str)],
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let dir = tempfile::TempDir::new()?;
    let (workspace, runner_temp) = (dir.path().join("ws"), dir.path().join("tmp"));
    for (rel, text) in files {
        let path = workspace.join(rel);
        fs::create_dir_all(path.parent().ok_or("no parent")?)?;
        fs::write(path, text)?;
    }
    fs::create_dir_all(&runner_temp)?;
    let mise_data = dir.path().join("mise-data");
    crate::impl_common::write_fixture_mise(&mise_data)?;
    let github_env = dir.path().join("github-env");
    let mut env = BTreeMap::new();
    for script in &generated.0 {
        fs::write(&github_env, "")?;
        let output = Command::new("/bin/bash")
            .args([
                "--noprofile",
                "--norc",
                "-p",
                "-e",
                "-o",
                "pipefail",
                "-c",
                script,
            ])
            .current_dir(&workspace)
            .env("PATH", VERIFY_PATH)
            .env("GITHUB_ENV", &github_env)
            .env("RUNNER_TEMP", &runner_temp)
            .env("MISE_DATA_DIR", &mise_data)
            .env("RUNNER_OS", "Linux")
            .env("RUNNER_ARCH", "X64")
            .env("CI", "true")
            .envs(credential_scrub())
            .output()
            .map_err(|err| format!("the identity tests need bash and mise on PATH: {err}"))?;
        assert!(output.status.success(), "{output:?}\n{script}");
        for line in fs::read_to_string(&github_env)?.lines() {
            let (name, value) = line.split_once('=').ok_or("bad GITHUB_ENV line")?;
            if let Some(previous) = env.insert(name.to_owned(), value.to_owned()) {
                assert_eq!(previous, value, "resolvers disagree on {name}");
            }
        }
    }
    let mut filled = Vec::new();
    for input in &generated.1 {
        let mut value = input.clone();
        for (name, resolved) in &env {
            value = value.replace(&format!("${{{{ env.{name} }}}}"), resolved);
        }
        assert!(!value.contains("${{"), "unresolved identity input {value}");
        filled.push(value);
    }
    Ok(filled)
}

type Generated = (Vec<String>, Vec<String>);

#[path = "impl_tools_identity_security.rs"]
mod security;

fn checkout(toml: &str, lock: Option<&str>, config: Option<&str>) -> Vec<(&'static str, String)> {
    let mut files = vec![("mise.toml", toml.to_owned())];
    files.extend(lock.map(|lock| ("mise.lock", lock.to_owned())));
    files.extend(config.map(|config| (".config/mise.toml", config.to_owned())));
    files
}

fn run(
    generated: &Generated,
    files: &[(&'static str, String)],
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(rel, text)| (*rel, text.as_str()))
        .collect();
    identities(generated, &files)
}

fn base_toml() -> String {
    mise_toml("0.9.148", "20.1.0", "a")
}

fn base_lock() -> String {
    mise_lock("0.9.148", "11", "22", "33", "20.1.0")
}

/// `mise.toml` without its Nextest line, which `.config/mise.toml` declares.
fn without_nextest() -> String {
    let toml = base_toml();
    let stripped = toml.replace(
        "\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.148\"\n",
        "",
    );
    assert_ne!(stripped, toml);
    stripped
}

fn config(nextest: &str) -> String {
    format!("[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"{nextest}\"\n")
}

/// Generated scripts and inputs, plus the identities of the base checkout.
fn baseline() -> Result<(Generated, Vec<String>), Box<dyn std::error::Error>> {
    let generated = generated()?;
    assert!(!generated.0.is_empty(), "no resolver step rendered");
    assert!(
        generated
            .1
            .iter()
            .any(|d| d.contains("${{ env.VELNOR_TOOL_NEXTEST_LOCK }}")),
        "{:?}",
        generated.1
    );
    for input in &generated.1 {
        assert!(!input.contains("hashFiles"), "{input}");
        assert!(!input.contains("NODE"), "{input}");
    }
    let base = run(
        &generated,
        &checkout(&base_toml(), Some(&base_lock()), None),
    )?;
    assert!(base.iter().any(|d| d.contains("-0.9.148-")), "{base:?}");
    Ok((generated, base))
}

#[test]
fn unrelated_mise_edits_keep_every_tools_identity() -> TestResult {
    let (generated, base) = baseline()?;
    let lock = base_lock();
    let cases = [
        (
            "task edit",
            checkout(&mise_toml("0.9.148", "20.1.0", "b"), Some(&lock), None),
        ),
        (
            "comment edit",
            checkout(
                &base_toml().replace("# project tools", "# other words"),
                Some(&lock),
                None,
            ),
        ),
        (
            "non-installed tool pin and lock",
            checkout(
                &mise_toml("0.9.148", "22.0.0", "a"),
                Some(&mise_lock("0.9.148", "11", "22", "33", "22.0.0")),
                None,
            ),
        ),
        (
            "another platform's lock row",
            checkout(
                &base_toml(),
                Some(&mise_lock("0.9.148", "11", "22", "34", "20.1.0")),
                None,
            ),
        ),
        (
            "same version declared in .config/mise.toml",
            checkout(&without_nextest(), Some(&lock), Some(&config("0.9.148"))),
        ),
    ];
    for (label, files) in cases {
        assert_eq!(run(&generated, &files)?, base, "{label}");
    }
    Ok(())
}

#[test]
fn installed_tool_versions_and_lock_rows_change_the_tools_identity() -> TestResult {
    let (generated, base) = baseline()?;
    let bumped_lock = mise_lock("0.9.149", "11", "22", "33", "20.1.0");
    let cases = [
        (
            "installed tool version bump",
            checkout(
                &mise_toml("0.9.149", "20.1.0", "a"),
                Some(&bumped_lock),
                None,
            ),
        ),
        (
            "installed tool bump in .config/mise.toml",
            checkout(
                &without_nextest(),
                Some(&bumped_lock),
                Some(&config("0.9.149")),
            ),
        ),
        (
            "installed tool lock checksum",
            checkout(
                &base_toml(),
                Some(&mise_lock("0.9.148", "12", "22", "33", "20.1.0")),
                None,
            ),
        ),
        (
            "Rust lock checksum",
            checkout(
                &base_toml(),
                Some(&mise_lock("0.9.148", "11", "23", "33", "20.1.0")),
                None,
            ),
        ),
        ("no lock: version alone", checkout(&base_toml(), None, None)),
    ];
    for (label, files) in cases {
        assert_ne!(run(&generated, &files)?, base, "{label}");
    }
    Ok(())
}
