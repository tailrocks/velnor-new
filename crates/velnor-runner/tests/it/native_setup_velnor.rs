// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::exec_support::Scratch;
use crate::mise_world::{SHA_PLACEHOLDER, World, assert_failed, job_fixture, real_action};
use crate::native_checkout::{Rig, finish, git};

const REAL_ERROR: &str =
    "::error title=setup-velnor::version 'x' must be MAJOR.MINOR.PATCH, digits only";
const ACTION: &str = "actions/setup-velnor/action.yml";

fn real_job(world: &World, fixture: &str, reference: &str) -> Result<PathBuf> {
    world.with_text(&job_fixture(fixture), &[(SHA_PLACEHOLDER, reference)])
}

#[test]
fn setup_velnor_reads_the_pinned_commit_despite_a_replace_ref() -> Result<()> {
    let world = World::new()?;
    let sha = world.publish_action(&real_action()?)?;
    let mirror = world.mirror();
    let original = git(&mirror, &["rev-parse", &format!("{sha}:{ACTION}")])?;
    let poisoned = real_action()?.replace("must be MAJOR.MINOR.PATCH", "POISONED-BY-REPLACE-REF");
    let staged = mirror.with_extension("poisoned.yml");
    std::fs::write(&staged, poisoned).context("write the poisoned action")?;
    let planted = git(&mirror, &["hash-object", "-w", &staged.to_string_lossy()])?;
    git(&mirror, &["replace", &original, &planted])?;
    assert!(
        git(&mirror, &["show", &format!("{sha}:{ACTION}")])?.contains("POISONED-BY-REPLACE-REF"),
        "the replace ref is planted"
    );
    let ran = world.run(&real_job(&world, "setup_velnor_real_action", &sha)?, &[])?;
    assert_failed(&ran, REAL_ERROR);
    assert!(!ran.stdout.contains("POISONED"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn setup_velnor_reads_the_mirror_that_checkout_filled() -> Result<()> {
    let rig = Rig::new()?;
    let work = rig.root.join("work");
    let directory = work.join("actions/setup-velnor");
    std::fs::create_dir_all(&directory).context("create the action directory")?;
    std::fs::write(directory.join("action.yml"), real_action()?).context("write the action")?;
    git(&work, &["add", "actions"])?;
    git(&work, &["commit", "-q", "-m", "action"])?;
    let upstream = rig.root.join("up/tailrocks/velnor-new.git");
    git(&work, &["push", "-q", &upstream.to_string_lossy(), "main"])?;
    let sha = git(&work, &["rev-parse", "HEAD"])?;
    let checkout = rig.job("checkout_depth1", &sha, &rig.server_url())?;
    let world = World::new()?;
    let setup = real_job(&world, "setup_velnor_real_action", &sha)?;
    let mut job: Value = serde_json::from_slice(&std::fs::read(&checkout)?)?;
    let mut second: Value = serde_json::from_slice(&std::fs::read(&setup)?)?;
    second["Steps"][0]["Id"] = "00000000-0000-0000-0000-0000000000f2".into();
    let steps = job["Steps"].as_array_mut().context("the checkout steps")?;
    steps.push(second["Steps"][0].clone());
    let combined = rig.root.join("combined.json");
    std::fs::write(&combined, serde_json::to_vec(&job)?)?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&combined, &scratch)?)?;
    assert!(ran.has_line("step 1 succeeded"), "{}", ran.stdout);
    assert!(ran.has_line(REAL_ERROR), "{}", ran.stdout);
    assert!(rig.mirror().is_dir());
    assert!(!rig.store().join("git/velnor-new.git").exists());
    Ok(())
}

#[test]
fn setup_velnor_ignores_git_from_the_step_path() -> Result<()> {
    let world = World::new()?;
    let sha = world.publish_action(&real_action()?)?;
    let job = world.plant_fake_git(&real_job(&world, "setup_velnor_real_action", &sha)?)?;
    let ran = world.run(&job, &[])?;
    assert!(ran.has_line(REAL_ERROR), "{}", ran.stdout);
    assert!(ran.has_line("job failed"), "{}", ran.stdout);
    assert!(!world.fake_git_marker().exists(), "the fake git ran");
    Ok(())
}

#[test]
fn setup_velnor_rejects_a_reference_that_is_not_a_commit_sha() -> Result<()> {
    let world = World::new()?;
    world.publish_action(&real_action()?)?;
    for reference in ["main", "abc123", &"g".repeat(40), &"a".repeat(41)] {
        let ran = world.run(
            &real_job(&world, "setup_velnor_real_action", reference)?,
            &[],
        )?;
        assert_failed(
            &ran,
            &format!("setup-velnor reference '{reference}' is not a 40 digit commit sha"),
        );
    }
    Ok(())
}

#[test]
fn setup_velnor_requires_its_required_input() -> Result<()> {
    let world = World::new()?;
    let sha = world.publish_action(&real_action()?)?;
    let ran = world.run(&real_job(&world, "setup_velnor_no_version", &sha)?, &[])?;
    assert_failed(&ran, "setup-velnor input 'version' is required");
    assert!(!ran.stdout.contains("setup-velnor step:"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn setup_velnor_rejects_a_nested_uses() -> Result<()> {
    let world = World::new()?;
    let action = format!("{}    - uses: actions/checkout@v4\n", real_action()?);
    let sha = world.publish_action(&action)?;
    let ran = world.run(&real_job(&world, "setup_velnor_real_action", &sha)?, &[])?;
    assert_failed(&ran, "unsupported: nested uses in setup-velnor");
    assert!(!ran.stdout.contains("setup-velnor step:"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn setup_velnor_fails_when_the_action_is_not_in_the_mirror() -> Result<()> {
    let world = World::new()?;
    let absent = "1".repeat(40);
    let git_dir = format!("--git-dir={}", world.mirror().display());
    let show = format!("show {absent}:{ACTION} failed: ");
    for published in [false, true] {
        if published {
            world.publish_action(&real_action()?)?;
        }
        let ran = world.run(&real_job(&world, "setup_velnor_real_action", &absent)?, &[])?;
        assert!(
            ran.lines()
                .iter()
                .any(|line| line.contains(&git_dir) && line.contains(&show)),
            "{git_dir} {show}\n{}",
            ran.stdout
        );
        assert!(ran.has_line("step 1 failed"), "{}", ran.stdout);
        assert!(!ran.stdout.contains("setup-velnor step:"), "{}", ran.stdout);
    }
    Ok(())
}

#[test]
fn setup_velnor_rejects_step_fields_it_does_not_implement() -> Result<()> {
    for field in [
        "if: false",
        "working-directory: sub",
        "id: install",
        "continue-on-error: true",
    ] {
        let world = World::new()?;
        let action = real_action()?.replace(
            "      shell: bash\n",
            &format!("      shell: bash\n      {field}\n"),
        );
        assert_ne!(action, real_action()?, "{field}");
        let sha = world.publish_action(&action)?;
        let ran = world.run(&real_job(&world, "setup_velnor_real_action", &sha)?, &[])?;
        let name = field.split(':').next().unwrap_or(field);
        assert!(
            ran.lines()
                .iter()
                .any(|text| text.starts_with("setup-velnor action.yml: ")
                    && text.contains(&format!("unknown field `{name}`"))),
            "{field}\n{}",
            ran.stdout
        );
        assert!(ran.has_line("step 1 failed"), "{field}\n{}", ran.stdout);
        assert!(!ran.stdout.contains("setup-velnor step:"), "{}", ran.stdout);
    }
    Ok(())
}

#[test]
fn setup_velnor_rejects_a_second_step() -> Result<()> {
    let world = World::new()?;
    let action = format!(
        "{}    - name: Second\n      shell: bash\n      run: echo second-ran\n",
        real_action()?
    );
    let sha = world.publish_action(&action)?;
    let ran = world.run(&real_job(&world, "setup_velnor_real_action", &sha)?, &[])?;
    assert_failed(
        &ran,
        "unsupported: setup-velnor with more than one step: GITHUB_ENV and GITHUB_PATH changes do not reach later steps of the same action",
    );
    assert!(!ran.stdout.contains("setup-velnor step:"), "{}", ran.stdout);
    assert!(!ran.stdout.contains("second-ran"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn setup_velnor_is_unsupported_outside_its_own_repository() -> Result<()> {
    let world = World::new()?;
    let sha = world.publish_action(&real_action()?)?;
    let fork = world.mirror().with_file_name("../../someone/fork.git");
    std::fs::create_dir_all(fork.parent().context("the fork parent")?)?;
    git(
        &world.mirror(),
        &["clone", "--quiet", "--bare", ".", &fork.to_string_lossy()],
    )?;
    let job = world.with_text(
        &job_fixture("setup_velnor_real_action"),
        &[
            (SHA_PLACEHOLDER, &sha),
            ("\"s\": \"tailrocks/velnor-new\"", "\"s\": \"someone/fork\""),
        ],
    )?;
    let ran = world.run(&job, &[])?;
    assert_failed(
        &ran,
        "unsupported: setup-velnor is only supported in tailrocks/velnor-new, not someone/fork",
    );
    assert!(!ran.stdout.contains("setup-velnor step:"), "{}", ran.stdout);
    Ok(())
}

#[test]
fn setup_velnor_rejects_an_action_key_it_does_not_implement() -> Result<()> {
    let world = World::new()?;
    let action = format!("{}outputs:\n  version:\n    value: x\n", real_action()?);
    let sha = world.publish_action(&action)?;
    let ran = world.run(&real_job(&world, "setup_velnor_real_action", &sha)?, &[])?;
    assert!(ran.has_line("job failed"), "{}", ran.stdout);
    assert!(
        ran.stdout.contains("unknown field `outputs`"),
        "{}",
        ran.stdout
    );
    assert!(!ran.stdout.contains("setup-velnor step:"), "{}", ran.stdout);
    Ok(())
}
