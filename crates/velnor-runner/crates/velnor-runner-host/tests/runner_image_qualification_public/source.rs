// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use super::*;

fn git(root: &Path, args: &[&str]) -> TestResult<()> {
    let output = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned().into())
    }
}

fn submodule_fixture(empty: bool) -> TestResult<(GitFixture, tempfile::TempDir)> {
    let mut fixture = GitFixture::new()?;
    let external = tempfile::tempdir()?;
    git(external.path(), &["init", "--quiet"])?;
    git(external.path(), &["config", "user.name", "Fixture"])?;
    git(
        external.path(),
        &["config", "user.email", "fixture@example.invalid"],
    )?;
    if !empty {
        fs::write(external.path().join("tracked"), "original\n")?;
        fs::write(
            external.path().join(".gitattributes"),
            "tracked filter=mask\n",
        )?;
        git(external.path(), &["add", "tracked", ".gitattributes"])?;
    }
    git(
        external.path(),
        &["commit", "--allow-empty", "--quiet", "-m", "source"],
    )?;
    git(
        fixture.root(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "--quiet",
            external.path().to_str().ok_or("path")?,
            "sub",
        ],
    )?;
    fixture.commit_all()?;
    Ok((fixture, external))
}

fn assert_rejected(fixture: &GitFixture) -> TestResult<()> {
    let (output, state) = qualify(
        fixture,
        fixture.source_sha(),
        ApiPlan::new(fixture.source_sha()),
    )?;
    assert_source_rejected_before_api(&output, &state);
    Ok(())
}

#[test]
fn qualifier_rejects_dirty_submodule_even_when_local_config_ignores_it() -> TestResult<()> {
    let (fixture, _external) = submodule_fixture(false)?;
    git(
        fixture.root(),
        &["submodule", "deinit", "--force", "--", "sub"],
    )?;
    let (output, state) = qualify(
        &fixture,
        fixture.source_sha(),
        ApiPlan::new(fixture.source_sha()),
    )?;
    assert_success(&output, &state);
    fs::write(fixture.root().join("sub/untracked"), "unexpected\n")?;
    assert_rejected(&fixture)?;
    fs::remove_file(fixture.root().join("sub/untracked"))?;
    git(
        fixture.root(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "update",
            "--init",
            "--",
            "sub",
        ],
    )?;
    git(fixture.root(), &["config", "submodule.sub.ignore", "all"])?;
    let (output, state) = qualify(
        &fixture,
        fixture.source_sha(),
        ApiPlan::new(fixture.source_sha()),
    )?;
    assert_success(&output, &state);
    fs::write(fixture.root().join("sub/tracked"), "altered\n")?;
    assert_rejected(&fixture)?;
    let sub = fixture.root().join("sub");
    fs::write(sub.join("tracked"), "original\n")?;
    git(&sub, &["config", "core.filemode", "false"])?;
    fs::set_permissions(sub.join("tracked"), fs::Permissions::from_mode(0o755))?;
    assert_rejected(&fixture)?;
    fs::set_permissions(sub.join("tracked"), fs::Permissions::from_mode(0o644))?;
    for (set, clear) in [
        ("--assume-unchanged", "--no-assume-unchanged"),
        ("--skip-worktree", "--no-skip-worktree"),
    ] {
        fs::write(sub.join("tracked"), "original\n")?;
        git(&sub, &["update-index", set, "tracked"])?;
        fs::write(sub.join("tracked"), "altered\n")?;
        let (output, state) = qualify(
            &fixture,
            fixture.source_sha(),
            ApiPlan::new(fixture.source_sha()),
        )?;
        assert_source_rejected_before_api(&output, &state);
        git(&sub, &["update-index", clear, "tracked"])?;
    }
    git(
        &sub,
        &["config", "filter.mask.clean", "sed s/altered/original/"],
    )?;
    fs::write(sub.join("tracked"), "altered\n")?;
    assert_rejected(&fixture)?;
    Ok(())
}

#[test]
fn qualifier_accepts_initialized_empty_submodule() -> TestResult<()> {
    let (fixture, _external) = submodule_fixture(true)?;
    let (output, state) = qualify(
        &fixture,
        fixture.source_sha(),
        ApiPlan::new(fixture.source_sha()),
    )?;
    assert_success(&output, &state);
    Ok(())
}

#[test]
fn qualifier_ignores_only_tracked_repository_exclusion_rules() -> TestResult<()> {
    let mut fixture = GitFixture::new()?;
    fs::write(fixture.root().join(".gitignore"), "generated/\n")?;
    fixture.commit_all()?;
    fs::create_dir(fixture.root().join("generated"))?;
    fs::write(fixture.root().join("generated/cache"), "ignored\n")?;
    let (output, state) = qualify(
        &fixture,
        fixture.source_sha(),
        ApiPlan::new(fixture.source_sha()),
    )?;
    assert_success(&output, &state);
    let hidden = fixture.root().join("hidden.txt");
    fs::write(&hidden, "unexpected\n")?;
    fs::write(fixture.root().join(".git/info/exclude"), "hidden.txt\n")?;
    assert_rejected(&fixture)?;
    fs::write(fixture.root().join(".git/info/exclude"), "")?;
    let exclude = tempfile::NamedTempFile::new()?;
    fs::write(exclude.path(), "hidden.txt\n")?;
    git(
        fixture.root(),
        &[
            "config",
            "core.excludesFile",
            exclude.path().to_str().ok_or("exclude")?,
        ],
    )?;
    assert_rejected(&fixture)?;
    fs::remove_file(hidden)?;
    fs::create_dir(fixture.root().join("src"))?;
    fs::write(fixture.root().join("src/.gitignore"), "*\n")?;
    fs::write(fixture.root().join("src/hidden"), "unexpected\n")?;
    assert_rejected(&fixture)?;
    Ok(())
}

#[test]
fn qualifier_ignores_untrusted_git_on_path() -> TestResult<()> {
    let fixture = GitFixture::new()?;
    let temp = tempfile::tempdir()?;
    let fake = temp.path().join("git");
    let marker = temp.path().join("called");
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\n: > '{}'\nfor arg in \"$@\"; do [ \"$arg\" != diff ] || exit 0; done\nexec /usr/bin/git \"$@\"\n",
            marker.display()
        ),
    )?;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700))?;
    let path = format!("{}:/usr/bin:/bin", temp.path().display());
    for dirty in [false, true] {
        if dirty {
            fixture.change_tracked()?;
        }
        let api = DockerApiFixture::new(ApiPlan::new(fixture.source_sha()))?;
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
            .env("PATH", &path)
            .args([
                "qualify-runner-images",
                fixture.root().to_str().ok_or("root")?,
                fixture.source_sha(),
                &digest_id('a'),
                &digest_id('b'),
                &digest_id('c'),
                api.socket().to_str().ok_or("socket")?,
                temp.path()
                    .join(if dirty { "dirty-result" } else { "result" })
                    .to_str()
                    .ok_or("result")?,
            ])
            .output()?;
        let state = api.snapshot()?;
        if dirty {
            assert_source_rejected_before_api(&output, &state);
        } else {
            assert_success(&output, &state);
        }
        assert!(!marker.exists());
    }
    Ok(())
}

#[test]
fn qualifier_rejects_git_config_that_hides_tracked_changes() -> TestResult<()> {
    let mut fixture = GitFixture::new()?;
    fs::write(
        fixture.root().join(".gitattributes"),
        "tracked.txt filter=mask\n",
    )?;
    fixture.commit_all()?;
    let (output, state) = qualify(
        &fixture,
        fixture.source_sha(),
        ApiPlan::new(fixture.source_sha()),
    )?;
    assert_success(&output, &state);
    git(fixture.root(), &["config", "core.filemode", "false"])?;
    let tracked = fixture.root().join("tracked.txt");
    fs::set_permissions(&tracked, fs::Permissions::from_mode(0o755))?;
    assert_rejected(&fixture)?;
    fs::set_permissions(&tracked, fs::Permissions::from_mode(0o644))?;
    git(
        fixture.root(),
        &["config", "filter.mask.clean", "sed s/altered/parent/"],
    )?;
    fs::write(tracked, "altered fixture\n")?;
    assert_rejected(&fixture)?;
    Ok(())
}

#[test]
fn qualifier_rejects_untrusted_git_attribute_sources() -> TestResult<()> {
    let mut fixture = GitFixture::new()?;
    fs::write(fixture.root().join(".gitignore"), "ignored/\n")?;
    fs::write(fixture.root().join(".gitattributes"), "tracked.txt text\n")?;
    fs::write(fixture.root().join("ident.txt"), "$Id$\n")?;
    fs::create_dir(fixture.root().join("ignored"))?;
    fs::write(fixture.root().join("ignored/ident"), "$Id$\n")?;
    git(fixture.root(), &["add", "-f", "ignored/ident"])?;
    fixture.commit_all()?;
    let (output, state) = qualify(
        &fixture,
        fixture.source_sha(),
        ApiPlan::new(fixture.source_sha()),
    )?;
    assert_success(&output, &state);
    let ident = fixture.root().join("ident.txt");
    let attributes = fixture.root().join(".git/info/attributes");
    fs::write(&attributes, "ident.txt ident\n")?;
    fs::write(&ident, "$Id: forged $\n")?;
    assert_rejected(&fixture)?;
    fs::remove_file(attributes)?;
    let external = tempfile::NamedTempFile::new()?;
    fs::write(external.path(), "ident.txt ident\n")?;
    git(
        fixture.root(),
        &[
            "config",
            "core.attributesFile",
            external.path().to_str().ok_or("attributes")?,
        ],
    )?;
    assert_rejected(&fixture)?;
    fs::write(ident, "$Id$\n")?;
    fs::write(
        fixture.root().join("ignored/.gitattributes"),
        "ident ident\n",
    )?;
    fs::write(fixture.root().join("ignored/ident"), "$Id: forged $\n")?;
    assert_rejected(&fixture)?;
    Ok(())
}
