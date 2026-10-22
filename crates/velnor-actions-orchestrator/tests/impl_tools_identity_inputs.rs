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

/// Read one complete resolver script by its emitted step identity, rather
/// than assuming the old raw-script prefix. Trusted verifier jobs now add a
/// credential and fixed-PATH prelude before the resolver payload.
fn resolver_script(file: &str) -> Option<String> {
    let mut resolver_step = false;
    for line in file.lines().map(str::trim_start) {
        if line == "- name: Resolve project tool versions" {
            resolver_step = true;
            continue;
        }
        if !resolver_step {
            continue;
        }
        if let Some(raw) = line.strip_prefix("run: \"") {
            let raw = raw
                .strip_suffix("\" # zizmor: ignore[github-env]")
                .or_else(|| raw.strip_suffix('"'))?;
            return Some(unquote(raw));
        }
        if line.starts_with("- name:") {
            return None;
        }
    }
    None
}

fn is_trusted_resolver(script: &str) -> bool {
    let unset = velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude();
    let prefix = format!(
        "{unset} PATH={}; export PATH; set -eu",
        velnor_actions_workflow_renderer::VERIFY_PATH
    );
    script.starts_with(&prefix)
        && script.contains("mise_bin=\"${HOME:?missing HOME}/.local/share/mise/bin/mise\"")
        && !script.contains("velnor_mise() { mise --no-config")
}

/// Every resolver script and every identity `d` input across the tree.
fn generated() -> Result<(Vec<String>, Vec<String>), Box<dyn std::error::Error>> {
    let repo = make_repo(CONFIG)?;
    fs::write(
        repo.path().join("mise.toml"),
        mise_toml("0.9.148", "20.1.0", "a"),
    )?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
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
        if let Some(script) = resolver_script(&file.bytes) {
            assert!(
                script.contains("velnor_tool VELNOR_TOOL_"),
                "resolver step has no tool binding in {}:\n{script}",
                file.path
            );
            assert!(
                is_trusted_resolver(&script),
                "resolver has an unexpected trust prelude in {}:\n{script}",
                file.path
            );
            scripts.push(script);
        }
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
    let home = dir.path().join("home");
    let trusted_mise = home.join(".local/share/mise/bin/mise");
    for (rel, text) in files {
        let path = workspace.join(rel);
        fs::create_dir_all(path.parent().ok_or("no parent")?)?;
        fs::write(path, text)?;
    }
    fs::create_dir_all(&runner_temp)?;
    fs::create_dir_all(trusted_mise.parent().ok_or("no trusted Mise parent")?)?;
    let real_mise = executable_on_path("mise")?;
    write_executable(&trusted_mise, &mise_delegate_wrapper(&real_mise))?;
    let github_env = dir.path().join("github-env");
    let mut env = BTreeMap::new();
    for script in &generated.0 {
        fs::write(&github_env, "")?;
        let output = Command::new("bash")
            .args(["--noprofile", "--norc", "-eo", "pipefail", "-c", script])
            .current_dir(&workspace)
            .env("GITHUB_ENV", &github_env)
            .env("RUNNER_TEMP", &runner_temp)
            .env("HOME", &home)
            .env("RUNNER_OS", "Linux")
            .env("RUNNER_ARCH", "X64")
            .env("CI", "true")
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
        generated.0.iter().all(|script| is_trusted_resolver(script)),
        "every dynamically-bound resolver must use its trusted shell and Mise path"
    );
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

#[cfg(unix)]
struct ResolverRuntime {
    home: std::path::PathBuf,
    workspace: std::path::PathBuf,
    runner_temp: std::path::PathBuf,
    poison: std::path::PathBuf,
    audit: std::path::PathBuf,
    poison_log: std::path::PathBuf,
}

#[cfg(unix)]
fn resolver_runtime(root: &std::path::Path) -> Result<ResolverRuntime, Box<dyn std::error::Error>> {
    let home = root.join("home");
    let mise_dir = home.join(".local/share/mise/bin");
    let workspace = root.join("workspace");
    let runner_temp = root.join("runner-temp");
    let poison = root.join("poison");
    fs::create_dir_all(&mise_dir)?;
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&runner_temp)?;
    fs::create_dir_all(&poison)?;
    fs::write(workspace.join("mise.toml"), base_toml())?;
    fs::write(workspace.join("mise.lock"), base_lock())?;

    let real_mise = executable_on_path("mise")?;
    let audit = root.join("mise-audit.log");
    write_executable(
        &mise_dir.join("mise"),
        &mise_audit_wrapper(&real_mise, &audit),
    )?;
    let poison_log = root.join("poison.log");
    let helpers = [
        "awk",
        "cp",
        "cut",
        "grep",
        "mkdir",
        "rm",
        "sha256sum",
        "shasum",
        "tr",
        "mise",
    ];
    for name in helpers {
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' '{}' >> {}\nexit 91\n",
            name,
            shell_quote(&poison_log.display().to_string())
        );
        write_executable(&poison.join(name), &script)?;
    }
    Ok(ResolverRuntime {
        home,
        workspace,
        runner_temp,
        poison,
        audit,
        poison_log,
    })
}

#[cfg(unix)]
fn run_resolver_script(
    runtime: &ResolverRuntime,
    index: usize,
    script: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let github_env = runtime.workspace.join(format!("github-env-{index}"));
    let output = Command::new("/bin/bash")
        .args([
            "--noprofile",
            "--norc",
            "-e",
            "-o",
            "pipefail",
            "-c",
            script,
        ])
        .current_dir(&runtime.workspace)
        .env("PATH", &runtime.poison)
        .env("HOME", &runtime.home)
        .env("GITHUB_ENV", &github_env)
        .env("RUNNER_TEMP", &runtime.runner_temp)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("CI", "true")
        .env("RESOLVER_AUDIT", &runtime.audit)
        .envs(
            velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS
                .iter()
                .map(|name| (*name, "should-never-reach-mise")),
        )
        .output()?;
    assert!(
        output.status.success(),
        "resolver {index} failed: {}\n{script}",
        String::from_utf8_lossy(&output.stderr)
    );
    let resolved = fs::read_to_string(github_env)?;
    assert!(resolved.contains("VELNOR_TOOL_"), "{resolved}");
    Ok(())
}

#[cfg(unix)]
fn assert_resolver_credentials_were_scrubbed(
    runtime: &ResolverRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    let audit = fs::read_to_string(&runtime.audit)?;
    for name in velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS {
        assert!(
            audit.lines().any(|line| line == format!("{name}=unset")),
            "Mise observed {name} before unset: {audit}"
        );
    }
    assert!(audit.contains(&format!(
        "MISE_DATA_DIR={}",
        runtime.home.join(".local/share/mise").display()
    )));
    assert!(
        !runtime.poison_log.exists(),
        "a helper resolved through poisoned PATH"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn emitted_resolvers_use_the_trusted_mise_and_scrub_credentials_before_helpers() -> TestResult {
    let root = tempfile::TempDir::new()?;
    let runtime = resolver_runtime(root.path())?;
    let scripts = generated()?.0;
    assert!(!scripts.is_empty(), "at least one resolver is emitted");
    for (index, script) in scripts.iter().enumerate() {
        run_resolver_script(&runtime, index, script)?;
    }
    assert_resolver_credentials_were_scrubbed(&runtime)?;
    Ok(())
}

#[cfg(unix)]
fn executable_on_path(name: &str) -> std::io::Result<std::path::PathBuf> {
    use std::{env, io};

    let paths = env::var_os("PATH").ok_or_else(|| io::Error::other("PATH is unset"))?;
    for directory in env::split_paths(&paths) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("{name} was not found on the test PATH"),
    ))
}

#[cfg(unix)]
fn write_executable(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::{fs, os::unix::fs::PermissionsExt};

    fs::write(path, contents)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

#[cfg(unix)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
fn mise_audit_wrapper(mise: &std::path::Path, audit: &std::path::Path) -> String {
    let mut credential_lines = String::new();
    for name in velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS {
        credential_lines.push_str("printf '%s=%s\\n' '");
        credential_lines.push_str(name);
        credential_lines.push_str("' \"${");
        credential_lines.push_str(name);
        credential_lines.push_str("-unset}\" >> \"$RESOLVER_AUDIT\"\n");
    }
    format!(
        "#!/bin/sh\nprintf 'args=%s\\n' \"$*\" >> {}\n{credential_lines}printf 'MISE_DATA_DIR=%s\\n' \"${{MISE_DATA_DIR-unset}}\" >> \"$RESOLVER_AUDIT\"\nexec {} \"$@\"\n",
        shell_quote(&audit.display().to_string()),
        shell_quote(&mise.display().to_string())
    )
}

#[cfg(unix)]
fn mise_delegate_wrapper(mise: &std::path::Path) -> String {
    format!(
        "#!/bin/sh\nexec {} \"$@\"\n",
        shell_quote(&mise.display().to_string())
    )
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
