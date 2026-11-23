// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Output;

use super::super::fixture::TestResult;
use super::scope::{assert_plain, assert_refused, declared_job, run_with_args};
use super::{plain_cargo, wrapper_command};
use crate::io::Repo;

fn aliases(launch: &Path, body: &str) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(launch.join(".cargo"))?;
    fs::write(launch.join(".cargo/config.toml"), body)?;
    Ok(())
}

fn manifests(repo: &Repo) -> (String, String) {
    let inside = repo.root().join("Cargo.toml").display().to_string();
    let outside = repo
        .root()
        .join("scratch-project/Cargo.toml")
        .display()
        .to_string();
    (inside, outside)
}

#[test]
fn the_built_in_aliases_and_read_manifest_are_scoped_by_the_project_they_name() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    for command in ["b", "c", "d", "t", "r", "rm", "read-manifest"] {
        assert_refused(&run_with_args(&repo, &launch, &[command, "-m", &inside])?);
        assert_refused(&run_with_args(
            &repo,
            &launch,
            &[command, &format!("-m{inside}")],
        )?);
        assert_plain(&run_with_args(&repo, &launch, &[command, "-m", &outside])?);
    }
    Ok(())
}

#[test]
fn a_user_alias_is_scoped_by_the_project_its_expansion_names() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    aliases(
        &launch,
        "[alias]\nxb = \"build --release\"\nxc = [\"check\", \"--all-targets\"]\nchained = \"xb\"\n",
    )?;
    let (inside, outside) = manifests(&repo);
    for alias in ["xb", "xc", "chained"] {
        assert_refused(&run_with_args(&repo, &launch, &[alias, "-m", &inside])?);
        assert_refused(&run_with_args(
            &repo,
            &launch,
            &[alias, "--manifest-path", &inside],
        )?);
        assert_plain(&run_with_args(&repo, &launch, &[alias, "-m", &outside])?);
    }
    Ok(())
}

#[test]
fn a_manifest_path_inside_an_alias_expansion_decides_the_project() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    aliases(
        &launch,
        &format!(
            "[alias]\nin = [\"build\", \"-m\", \"{inside}\"]\nout = [\"build\", \"-m\", \"{outside}\"]\n"
        ),
    )?;
    assert_refused(&run_with_args(&repo, &launch, &["in"])?);
    assert_plain(&run_with_args(&repo, &launch, &["out"])?);
    Ok(())
}

#[test]
fn an_alias_to_a_custom_subcommand_keeps_that_subcommands_own_flags() -> TestResult {
    let repo = declared_job()?;
    aliases(repo.root(), "[alias]\nnt = \"nextest run\"\n")?;
    let (_, outside) = manifests(&repo);
    assert_refused(&run_with_args(&repo, repo.root(), &["nt", "-m", &outside])?);
    assert_plain(&run_with_args(
        &repo,
        repo.root(),
        &["nt", "--manifest-path", &outside],
    )?);
    Ok(())
}

#[test]
fn an_alias_definition_that_cannot_be_read_fails_closed_for_an_unknown_subcommand() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    for body in ["[alias\n", "include = [\"other.toml\"]\n"] {
        aliases(&launch, body)?;
        for flag in ["-m", "--manifest-path"] {
            assert_refused(&run_with_args(&repo, &launch, &["xb", flag, &outside])?);
            assert_refused(&run_with_args(&repo, &launch, &["xb", flag, &inside])?);
        }
        assert_plain(&run_with_args(
            &repo,
            &launch,
            &["metadata", "-m", &outside],
        )?);
    }
    Ok(())
}

#[test]
fn an_alias_given_on_the_command_line_fails_closed() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (_, outside) = manifests(&repo);
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["--config", "alias.xb='build'", "xb", "-m", &outside],
    )?);
    Ok(())
}

#[test]
fn a_user_alias_overrides_a_built_in_alias_of_the_same_name() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    aliases(&launch, &format!("[alias]\nb = \"build -m {inside}\"\n"))?;
    assert_refused(&run_with_args(&repo, &launch, &["b"])?);
    aliases(&launch, &format!("[alias]\nb = \"build -m {outside}\"\n"))?;
    assert_plain(&run_with_args(&repo, &launch, &["b"])?);
    Ok(())
}

#[test]
fn the_extensionless_cargo_config_wins_over_config_toml() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    aliases(&launch, &format!("[alias]\nxx = \"build -m {outside}\"\n"))?;
    fs::write(
        launch.join(".cargo/config"),
        format!("[alias]\nxx = \"build -m {inside}\"\n"),
    )?;
    assert_refused(&run_with_args(&repo, &launch, &["xx"])?);
    Ok(())
}

#[test]
fn a_dotted_alias_name_is_read_from_its_environment_variable() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    let plain = plain_cargo(&repo)?;
    fs::create_dir_all(&launch)?;
    let run = |value: &str| -> Result<Output, Box<dyn std::error::Error>> {
        Ok(
            wrapper_command(repo.root(), &plain, true, &[("CARGO_ALIAS_FOO_BAR", value)])?
                .current_dir(&launch)
                .args(["foo.bar"])
                .output()?,
        )
    };
    assert_refused(&run(&format!("build -m {inside}"))?);
    assert_plain(&run(&format!("build -m {outside}"))?);
    Ok(())
}

#[test]
fn a_user_alias_never_overrides_a_real_built_in_command() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    aliases(
        &launch,
        &format!("[alias]\ntest = \"metadata --no-deps --format-version 1 -m {outside}\"\n"),
    )?;
    assert_refused(&run_with_args(&repo, &launch, &["test", "-m", &inside])?);
    assert_plain(&run_with_args(&repo, &launch, &["test", "-m", &outside])?);
    Ok(())
}

#[test]
fn a_user_alias_never_overrides_any_other_built_in_command() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, _) = manifests(&repo);
    for name in ["new", "version"] {
        aliases(
            &launch,
            &format!("[alias]\n{name} = \"build -m {inside}\"\n"),
        )?;
        assert_plain(&run_with_args(&repo, &launch, &[name])?);
    }
    Ok(())
}

#[test]
fn an_included_override_of_a_short_alias_fails_closed() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    aliases(&launch, "include = [\"aliases.toml\"]\n")?;
    fs::write(
        launch.join(".cargo/aliases.toml"),
        format!("[alias]\nb = \"build -m {inside}\"\n"),
    )?;
    assert_refused(&run_with_args(&repo, &launch, &["b"])?);
    assert_refused(&run_with_args(&repo, &launch, &["b", "-m", &outside])?);
    Ok(())
}

#[test]
fn a_relative_cargo_home_is_read_from_the_directory_flag_directory() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, _) = manifests(&repo);
    let target = launch.join("elsewhere");
    fs::create_dir_all(target.join("relative-home"))?;
    fs::write(
        target.join("relative-home/config.toml"),
        format!("[alias]\nb = \"build -m {inside}\"\n"),
    )?;
    let plain = plain_cargo(&repo)?;
    let output = wrapper_command(
        repo.root(),
        &plain,
        false,
        &[("CARGO_HOME", "relative-home")],
    )?
    .current_dir(&launch)
    .env("VELNOR_CARGO_MBX", "required")
    .args(["-C", "elsewhere", "b"])
    .output()?;
    assert_refused(&output);
    Ok(())
}

#[test]
fn an_alias_named_like_a_script_path_does_not_replace_the_script_target() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (_, outside) = manifests(&repo);
    let script = repo.root().join("tool.rs").display().to_string();
    aliases(
        &launch,
        &format!("[alias]\n\"{script}\" = \"build -m {outside}\"\n"),
    )?;
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", &script],
    )?);
    Ok(())
}

#[test]
fn an_unknown_custom_alias_fails_closed() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, _) = manifests(&repo);
    aliases(&launch, "include = [\"aliases.toml\"]\n")?;
    fs::write(
        launch.join(".cargo/aliases.toml"),
        format!("[alias]\nx = \"build --manifest-path {inside}\"\n"),
    )?;
    assert_refused(&run_with_args(&repo, &launch, &["x"])?);
    aliases(&launch, "[alias\n")?;
    assert_refused(&run_with_args(&repo, &launch, &["x"])?);
    Ok(())
}

#[test]
fn a_user_alias_named_like_any_cargo_built_in_is_ignored() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    aliases(
        &launch,
        &format!("[alias]\nsearch = \"build --manifest-path {inside}\"\n"),
    )?;
    assert_plain(&run_with_args(
        &repo,
        &launch,
        &["search", "--manifest-path", &outside],
    )?);
    Ok(())
}

#[test]
fn a_trailing_config_argument_of_an_external_subcommand_is_not_an_alias_definition() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    assert_plain(&run_with_args(
        &repo,
        &launch,
        &["foo", "--config", "custom.toml", "value"],
    )?);
    Ok(())
}

#[test]
fn a_relative_home_is_resolved_from_the_directory_flag() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let target = repo.root().join("target");
    let (inside, outside) = manifests(&repo);
    for (directory, manifest) in [(&launch, &outside), (&target, &inside)] {
        let config = directory.join("relative-home/.cargo");
        fs::create_dir_all(&config)?;
        fs::write(
            config.join("config.toml"),
            format!("[alias]\nx = \"build --manifest-path {manifest}\"\n"),
        )?;
    }
    fs::create_dir_all(&launch)?;
    let output = wrapper_command(repo.root(), &plain_cargo(&repo)?, false, &[])?
        .current_dir(&launch)
        .env_remove("CARGO_HOME")
        .env("HOME", "relative-home")
        .env("VELNOR_CARGO_MBX", "required")
        .args(["-C", &target.display().to_string(), "x"])
        .output()?;
    assert_refused(&output);
    Ok(())
}

#[test]
fn a_user_alias_named_add_info_remove_or_report_is_ignored_as_cargo_ignores_it() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, _) = manifests(&repo);
    for name in ["add", "info", "remove", "report"] {
        aliases(
            &launch,
            &format!("[alias]\n{name} = \"build -m {inside}\"\n"),
        )?;
        assert_plain(&run_with_args(&repo, &launch, &[name])?);
    }
    Ok(())
}

#[test]
fn an_alias_after_the_option_separator_is_scoped_by_its_expansion() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    aliases(&launch, &format!("[alias]\nxx = \"build -m {inside}\"\n"))?;
    assert_refused(&run_with_args(&repo, &launch, &["--", "xx"])?);
    aliases(&launch, &format!("[alias]\nxx = \"build -m {outside}\"\n"))?;
    assert_plain(&run_with_args(&repo, &launch, &["--", "xx"])?);
    Ok(())
}
