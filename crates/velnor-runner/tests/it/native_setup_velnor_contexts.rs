// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use crate::mise_world::{SHA_PLACEHOLDER, World, job_fixture, real_action};

const REAL_ERROR: &str =
    "::error title=setup-velnor::version 'x' must be MAJOR.MINOR.PATCH, digits only";
const STEP_NAME: &str = "- name: Install velnor-ci";

fn job(world: &World, sha: &str, env: &[(&str, &str)]) -> Result<PathBuf> {
    let job = world.with_text(
        &job_fixture("setup_velnor_real_action"),
        &[(SHA_PLACEHOLDER, sha)],
    )?;
    if env.is_empty() {
        Ok(job)
    } else {
        world.with_job_env(&job, env)
    }
}

#[test]
fn setup_velnor_renders_the_outer_contexts_of_its_step() -> Result<()> {
    let world = World::new()?;
    let name = "${{ github.repository }}-${{ github.workspace }}-${{ env.OUTER }}-${{ env.STEP_ENV }}-${{ env.FROM_FILE }}-${{ runner.temp }}-${{ inputs.version }}-[${{ steps.leak.outputs.v }}]";
    let action = real_action()?.replace(STEP_NAME, &format!("- name: {name}"));
    let sha = world.publish_action(&action)?;
    let outer = job(&world, &sha, &[("OUTER", "from-job")])?;
    let leading = world.with_leading_script(
        &outer,
        "echo v=leaked >> \"$GITHUB_OUTPUT\"\necho FROM_FILE=from-file >> \"$GITHUB_ENV\"",
    )?;
    let mut message: Value = serde_json::from_slice(&std::fs::read(&leading)?)?;
    message["Steps"][0]["ContextName"] = "leak".into();
    message["Steps"][1]["Environment"] = json!({"type": 2, "map": [
        {"key": {"type": 0, "lit": "STEP_ENV"}, "value": {"type": 0, "lit": "from-step"}}
    ]});
    std::fs::write(&leading, serde_json::to_vec(&message)?)?;
    let ran = world.run(&leading, &[])?;
    let expected = format!(
        "setup-velnor step: tailrocks/velnor-new-{}-from-job-from-step-from-file-{}-x-[]",
        world.workspace().display(),
        world.temp().display()
    );
    ensure!(
        ran.has_line(&expected) && ran.has_line(REAL_ERROR),
        "{expected}\n{}",
        ran.stdout
    );
    Ok(())
}

#[test]
fn setup_velnor_refuses_a_context_it_cannot_supply() -> Result<()> {
    for root in ["secrets", "needs", "matrix", "job", "strategy", "vars"] {
        let world = World::new()?;
        let action = real_action()?.replace(STEP_NAME, &format!("- name: ${{{{ {root}.value }}}}"));
        let sha = world.publish_action(&action)?;
        let ran = world.run(&job(&world, &sha, &[])?, &[])?;
        let expected =
            format!("unsupported: setup-velnor expression: unsupported expression context {root}");
        ensure!(
            ran.has_line(&expected) && ran.has_line("step 1 failed"),
            "{expected}\n{}",
            ran.stdout
        );
        ensure!(
            !ran.stdout.contains("setup-velnor step:"),
            "{root}\n{}",
            ran.stdout
        );
    }
    Ok(())
}

#[test]
fn setup_velnor_refuses_an_action_without_steps() -> Result<()> {
    let action = real_action()?;
    let header = action
        .split_once("  steps:")
        .map_or_default(|(header, _)| header.to_owned());
    ensure!(header.ends_with("  using: composite\n"), "{header}");
    for (label, text) in [
        ("missing", header.clone()),
        ("empty", format!("{header}  steps: []\n")),
    ] {
        let world = World::new()?;
        let sha = world.publish_action(&text)?;
        let ran = world.run(&job(&world, &sha, &[])?, &[])?;
        ensure!(
            ran.has_line("setup-velnor action.yml declares no steps")
                && ran.has_line("step 1 failed"),
            "{label}\n{}",
            ran.stdout
        );
        ensure!(
            !ran.stdout.contains("setup-velnor step:"),
            "{label}\n{}",
            ran.stdout
        );
    }
    Ok(())
}

fn header() -> Result<String> {
    let action = real_action()?;
    let (header, _) = action
        .split_once("  steps:")
        .context("the action has steps")?;
    Ok(header.to_owned())
}

#[test]
fn setup_velnor_renders_the_shell_of_its_step() -> Result<()> {
    let probe = |inner: &str| {
        Ok::<_, anyhow::Error>(format!(
            "{}  steps:\n    - name: Probe\n      shell: ${{{{ env.CHOSEN }}}}\n{inner}      run: echo \"opts=$SHELLOPTS\"\n",
            header()?
        ))
    };
    let inherited = probe("")?;
    let inner = |chosen: &str| probe(&format!("      env:\n        CHOSEN: {chosen}\n"));
    for (action, outer, chosen, pipefail) in [
        (inherited.clone(), "bash", "bash", true),
        (inherited, "sh", "sh", false),
        (inner("sh")?, "bash", "sh", false),
        (inner("bash")?, "sh", "bash", true),
    ] {
        let world = World::new()?;
        let sha = world.publish_action(&action)?;
        let ran = world.run(&job(&world, &sha, &[("CHOSEN", outer)])?, &[])?;
        let opts = ran
            .stdout
            .lines()
            .find(|line| line.starts_with("opts="))
            .context("the probe printed its options")?;
        ensure!(
            opts.contains("pipefail") == pipefail && ran.has_line("job succeeded"),
            "{chosen}: {opts}\n{}",
            ran.stdout
        );
    }
    Ok(())
}

#[test]
fn setup_velnor_merges_the_env_of_its_step_before_rendering_it() -> Result<()> {
    let world = World::new()?;
    let action = real_action()?
        .replace(
            "        VERSION: ${{ inputs.version }}\n",
            "        VERSION: ${{ inputs.version }}\n        INNER: from-composite\n",
        )
        .replace(STEP_NAME, "- name: ${{ env.INNER }}")
        .replace(
            "        set -euo pipefail\n",
            "        set -euo pipefail\n        echo \"script-sees=${{ env.INNER }}\"\n",
        );
    let sha = world.publish_action(&action)?;
    let ran = world.run(&job(&world, &sha, &[("INNER", "from-job")])?, &[])?;
    ensure!(
        ran.has_line("setup-velnor step: from-composite")
            && ran.has_line("script-sees=from-composite")
            && ran.has_line(REAL_ERROR),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[test]
fn setup_velnor_refuses_a_step_that_reads_the_action_path() -> Result<()> {
    let rows = [
        ("name", STEP_NAME, "- name: ${{ Github.ACTION_PATH }}"),
        ("shell", "shell: bash", "shell: ${{ github.action_path }}"),
        (
            "run",
            "set -euo pipefail\n",
            "set -euo pipefail\n        echo \"$GITHUB_ACTION_PATH\"\n",
        ),
        (
            "env",
            "VERSION: ${{ inputs.version }}",
            "VERSION: ${{ inputs.version }}\n        HERE: ${{ github.action_path }}",
        ),
    ];
    for (label, from, to) in rows {
        let world = World::new()?;
        let action = real_action()?.replace(from, to);
        ensure!(action != real_action()?, "{label}");
        let sha = world.publish_action(&action)?;
        let ran = world.run(&job(&world, &sha, &[])?, &[])?;
        let expected =
            "unsupported: setup-velnor step that reads github.action_path or GITHUB_ACTION_PATH";
        ensure!(
            ran.has_line(expected) && ran.has_line("step 1 failed"),
            "{label}: {expected}\n{}",
            ran.stdout
        );
        ensure!(
            !ran.stdout.contains("setup-velnor step:"),
            "{label}\n{}",
            ran.stdout
        );
    }
    Ok(())
}

#[test]
fn setup_velnor_allows_a_name_that_only_contains_action_path() -> Result<()> {
    let world = World::new()?;
    let action = real_action()?.replace(
        "set -euo pipefail\n",
        "set -euo pipefail\n        echo \"TRANSACTION_PATH=${TRANSACTION_PATH:-}\"\n",
    );
    ensure!(action != real_action()?, "the run row was not applied");
    let sha = world.publish_action(&action)?;
    let ran = world.run(&job(&world, &sha, &[])?, &[])?;
    ensure!(
        ran.has_line(REAL_ERROR) && !ran.stdout.contains("reads github.action_path"),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[test]
fn setup_velnor_refuses_an_input_default_that_is_an_expression() -> Result<()> {
    let world = World::new()?;
    let action = real_action()?.replacen("default: ''", "default: ${{ github.sha }}", 1);
    let sha = world.publish_action(&action)?;
    let ran = world.run(&job(&world, &sha, &[])?, &[])?;
    let expected =
        "unsupported: setup-velnor input 'sha256-linux-x64' has a default that is an expression";
    ensure!(
        ran.has_line(expected) && ran.has_line("step 1 failed"),
        "{expected}\n{}",
        ran.stdout
    );
    ensure!(!ran.stdout.contains("setup-velnor step:"), "{}", ran.stdout);
    Ok(())
}
