//! Hard decision 5: the generated resolver script is executed, not only
//! inspected. Each case writes a project checkout, runs the exact script
//! the renderer emits under `bash -e` with the real `mise` binary, and
//! asserts the `$GITHUB_ENV` content (or its absence).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use velnor_actions_contract::config::project_tool_resolver::project_tool_resolver_script;

use crate::impl_common::TestResult;

/// Result of one resolver run.
pub(crate) struct Run {
    pub(crate) ok: bool,
    pub(crate) env: String,
    pub(crate) stderr: String,
}

/// Run the generated resolver for `names` in a checkout holding `files`.
pub(crate) fn run(
    files: &[(&str, &str)],
    links: &[(&str, &str)],
    names: &[&str],
    rustup: bool,
) -> Result<Run, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("checkout");
    fs::create_dir_all(&root)?;
    for (rel, text) in files {
        write(&root, rel, text)?;
    }
    for (rel, target) in links {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        std::os::unix::fs::symlink(target, path)?;
    }
    let temp = dir.path().join("runner-temp");
    fs::create_dir_all(&temp)?;
    let mise_data = dir.path().join("mise-data");
    crate::impl_common::write_fixture_mise(&mise_data)?;
    let env_file = dir.path().join("github-env");
    fs::write(&env_file, "")?;
    let set: BTreeSet<String> = names.iter().map(|name| (*name).to_owned()).collect();
    let script = project_tool_resolver_script(&set, rustup);
    let output = Command::new("/bin/bash")
        .args([
            "--noprofile",
            "--norc",
            "-p",
            "-e",
            "-o",
            "pipefail",
            "-c",
            &script,
        ])
        .current_dir(&root)
        .env("GITHUB_ENV", &env_file)
        .env("RUNNER_TEMP", &temp)
        .env("MISE_DATA_DIR", &mise_data)
        // GitHub runners set CI=true, under which Mise auto-trusts config.
        .env("CI", "true")
        .output()
        .map_err(|err| format!("the resolver tests need bash and mise on PATH: {err}"))?;
    Ok(Run {
        ok: output.status.success(),
        env: fs::read_to_string(&env_file)?,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn write(root: &Path, rel: &str, text: &str) -> TestResult {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    Ok(())
}

pub(crate) const RUST: &str = "VELNOR_TOOL_RUST";

pub(crate) fn passes(files: &[(&str, &str)], names: &[&str], expected_env: &str) -> TestResult {
    let outcome = run(files, &[], names, false)?;
    assert!(outcome.ok, "{files:?} failed: {}", outcome.stderr);
    assert_eq!(outcome.env, expected_env, "{files:?}");
    Ok(())
}

pub(crate) fn fails(files: &[(&str, &str)], names: &[&str], needle: &str) -> TestResult {
    let outcome = run(files, &[], names, false)?;
    assert!(!outcome.ok, "{files:?} must fail");
    assert!(
        outcome.stderr.contains(needle),
        "{files:?}: expected `{needle}` in `{}`",
        outcome.stderr
    );
    assert_eq!(outcome.env, "", "{files:?} must not write $GITHUB_ENV");
    Ok(())
}

#[test]
fn exact_versions_resolve_from_every_mise_location() -> TestResult {
    for location in [
        "mise.toml",
        ".mise.toml",
        "mise.local.toml",
        "mise.ci.toml",
        ".mise.ci.toml",
        ".config/mise.toml",
        ".config/mise/config.toml",
        ".config/mise/config.local.toml",
        ".config/mise/conf.d/10-tools.toml",
        "mise/config.toml",
        ".mise/config.toml",
    ] {
        passes(
            &[(location, "[tools]\nrust = \"1.99.1\"\n")],
            &[RUST],
            "VELNOR_TOOL_RUST=1.99.1\n",
        )?;
    }
    passes(
        &[(".tool-versions", "# pins\nrust 1.99.1 # comment\njava 21\n")],
        &[RUST],
        "VELNOR_TOOL_RUST=1.99.1\n",
    )
}

#[test]
fn every_backend_spelling_and_table_form_resolves() -> TestResult {
    passes(
        &[(
            "mise.toml",
            "[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = { version = \"0.9.148\", os = [\"linux\"] }\n\"packslip:github.com/jdx/mr-boxington\" = \"1.25.0\"\n\n[tools.shellcheck]\nversion = \"0.11.0\"\npostinstall = \"echo\"\n",
        )],
        &[
            "VELNOR_TOOL_MR_BOXINGTON",
            "VELNOR_TOOL_NEXTEST",
            "VELNOR_TOOL_SHELLCHECK",
        ],
        "VELNOR_TOOL_MR_BOXINGTON=1.25.0\nVELNOR_TOOL_NEXTEST=0.9.148\nVELNOR_TOOL_SHELLCHECK=0.11.0\n",
    )?;
    passes(
        &[("mise.toml", "[tools]\n\"core:rust\" = \"1.99.1\"\n")],
        &[RUST],
        "VELNOR_TOOL_RUST=1.99.1\n",
    )
}

#[test]
fn crlf_files_resolve_but_a_carriage_return_inside_a_value_fails() -> TestResult {
    passes(
        &[("mise.toml", "[tools]\r\nrust = \"1.99.1\"\r\n")],
        &[RUST],
        "VELNOR_TOOL_RUST=1.99.1\n",
    )?;
    fails(
        &[("mise.toml", "[tools]\nrust = \"1.99.1\\r\"\n")],
        &[RUST],
        "exact X.Y.Z",
    )?;
    fails(
        &[(".tool-versions", "rust 1.99.1\r\n")],
        &[RUST],
        "exact X.Y.Z",
    )
}

#[test]
fn templates_never_execute_and_never_resolve() -> TestResult {
    let dir = tempfile::tempdir()?;
    let marker = dir.path().join("pwned");
    let hostile = format!(
        "[tools]\nrust = \"{{{{ exec(command='touch {}') }}}}\"\n[env]\n_.source = \"touch {}\"\n[hooks]\nenter = \"touch {}\"\n",
        marker.display(),
        marker.display(),
        marker.display()
    );
    fails(&[("mise.toml", &hostile)], &[RUST], "exact X.Y.Z")?;
    assert!(!marker.exists(), "the resolver executed project config");
    fails(
        &[("mise.toml", "[tools]\nrust = \"{{ env.X }}\"\n")],
        &[RUST],
        "exact X.Y.Z",
    )
}

#[test]
fn multiline_array_and_non_string_values_fail_without_injecting() -> TestResult {
    for value in [
        "\"\"\"\n1.99.1\nVELNOR_TOOL_X=1\"\"\"",
        "'''\n1.99.1\nVELNOR_TOOL_X=1'''",
        "[\"1.99.1\"]",
        "[\"1.99.1\", \"1.99.0\"]",
        "{ version = [\"1.99.1\"] }",
        "{ version = 1 }",
        "199",
        "true",
        "\"1.99.1\\nVELNOR_TOOL_X=1\"",
    ] {
        fails(
            &[("mise.toml", &format!("[tools]\nrust = {value}\n"))],
            &[RUST],
            "velnor:",
        )?;
    }
    // A multi-line string that merely contains key-shaped text declares nothing.
    passes(
        &[(
            "mise.toml",
            "[tools]\nshellcheck = \"\"\"\nrust = \"9.9.9\"\n\"\"\"\nrust = \"1.99.1\"\n",
        )],
        &[RUST],
        "VELNOR_TOOL_RUST=1.99.1\n",
    )
}

#[test]
fn loose_and_non_canonical_versions_fail() -> TestResult {
    for value in [
        "0.9",
        "1",
        "stable",
        "latest",
        "^1.99.1",
        "~1.99.1",
        "v1.99.1",
        "1.99.1.2",
        "1.99",
        "prefix:1.99",
        "ref:main",
        "path:/tmp/x",
        "1.99.1-x86_64-unknown-linux-gnu",
        "01.99.1",
        "1.099.1",
        "1.99.01",
        "1..1",
        ".1.1",
        "1.1.",
        "",
        "1.99.1 1.98.0",
        "1.99.1\\n",
    ] {
        fails(
            &[("mise.toml", &format!("[tools]\nrust = \"{value}\"\n"))],
            &[RUST],
            "exact X.Y.Z",
        )?;
    }
    passes(
        &[("mise.toml", "[tools]\nrust = \"0.0.0\"\n")],
        &[RUST],
        "VELNOR_TOOL_RUST=0.0.0\n",
    )
}

#[test]
fn error_messages_never_echo_control_text() -> TestResult {
    let outcome = run(
        &[("mise.toml", "[tools]\nrust = \"::error::boom\"\n")],
        &[],
        &[RUST],
        false,
    )?;
    assert!(!outcome.ok);
    assert!(
        outcome
            .stderr
            .lines()
            .all(|line| line.starts_with("velnor:")),
        "{}",
        outcome.stderr
    );
    Ok(())
}

#[test]
fn symlinked_sources_are_refused() -> TestResult {
    for link in [
        "mise.toml",
        ".config/mise.toml",
        ".config/mise/conf.d/x.toml",
        ".tool-versions",
        "rust-toolchain.toml",
        "rust-toolchain",
    ] {
        let outcome = run(
            &[("real.toml", "[tools]\nrust = \"1.99.1\"\n")],
            &[(link, "real.toml")],
            &[RUST],
            false,
        )?;
        assert!(!outcome.ok, "{link}");
        assert!(
            outcome.stderr.contains("regular file"),
            "{link}: {}",
            outcome.stderr
        );
        assert_eq!(outcome.env, "", "{link}");
    }
    let outcome = run(&[], &[("mise.toml", "missing-target")], &[RUST], false)?;
    assert!(!outcome.ok && outcome.stderr.contains("regular file"));
    Ok(())
}
