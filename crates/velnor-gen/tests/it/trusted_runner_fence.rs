// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, refusal, rendered_file, workflow};
use crate::pipeline::{pin_helper, view, workflow as rust_workflow};

const POOL: &str = "velnor-box-trusted";
const ON: &str = "[workflow.on.workflow_dispatch]\n";
const HEADER: &str = "runner = \"velnor-box\"\ntrusted_runner = \"velnor-box-trusted\"";

fn checkout_job(with: &str) -> String {
    format!(
        "\n[[workflow.job]]\nid = \"build\"\npermissions = {{ contents = \"read\" }}\n\n[[workflow.job.step]]\nname = \"Checkout\"\nuses = \"checkout\"\n\n[workflow.job.step.with]\n{with}\n"
    )
}

#[test]
fn a_checkout_key_in_another_case_is_refused_like_the_lowercase_one() -> TestResult {
    let on = "[workflow.on.workflow_dispatch]\n\n[[workflow.on.workflow_dispatch.input]]\nname = \"target\"\ntype = \"string\"\n";
    for with in [
        "Ref = \"${{ inputs.target }}\"",
        "REF = \"${{ inputs.target }}\"",
        "Repository = \"${{ inputs.target }}\"",
        "REPOSITORY = \"${{ github.event.inputs.target }}\"",
        "Ref = \"${{ steps.pick.outputs.ref }}\"",
        "REPOSITORY = \"${{ needs.plan.outputs.repo }}\"",
    ] {
        let message = refusal(&workflow(HEADER, on, &checkout_job(with)))?;
        assert!(message.contains("trusted_runner"), "{with}: {message}");
    }
    Ok(())
}

#[test]
fn a_checkout_that_reads_the_matrix_is_refused() -> TestResult {
    for with in [
        "ref = \"${{ matrix.ref }}\"",
        "repository = \"${{ matrix.repo }}\"",
        "Ref = \"${{ matrix.ref }}\"",
    ] {
        let message = refusal(&workflow(HEADER, ON, &checkout_job(with)))?;
        assert!(message.contains("trusted_runner"), "{with}: {message}");
        assert!(message.contains("matrix"), "{with}: {message}");
    }
    Ok(())
}

#[test]
fn a_checkout_that_overrides_the_server_url_is_refused() -> TestResult {
    for with in [
        "github-server-url = \"${{ inputs.server }}\"",
        "github-server-url = \"https://example.com\"",
        "GitHub-Server-URL = \"https://example.com\"",
    ] {
        let message = refusal(&workflow(HEADER, ON, &checkout_job(with)))?;
        assert!(message.contains("trusted_runner"), "{with}: {message}");
        assert!(message.contains("github-server-url"), "{with}: {message}");
    }
    Ok(())
}

#[test]
fn a_static_checkout_stays_accepted() -> TestResult {
    for with in [
        "ref = \"main\"",
        "ref = \"${{ github.sha }}\"",
        "fetch-depth = \"0\"",
    ] {
        rendered_file(&workflow(HEADER, ON, &checkout_job(with)), "ci")?;
    }
    Ok(())
}

#[test]
fn a_job_or_matrix_that_names_the_trusted_label_is_refused() -> TestResult {
    for jobs in [
        format!(
            "\n[[workflow.job]]\nid = \"build\"\nrunner = \"{POOL}\"\n\n[[workflow.job.step]]\nname = \"Build\"\nrun = \"echo ok\"\n"
        ),
        format!(
            "\n[[workflow.job]]\nid = \"build\"\nrunner = \"{}\"\n\n[[workflow.job.step]]\nname = \"Build\"\nrun = \"echo ok\"\n",
            POOL.to_uppercase()
        ),
        format!(
            "\n[[workflow.job]]\nid = \"build\"\nrunner = \"${{{{ matrix.runner }}}}\"\nrunner_labels = {{ \"{POOL}\" = false }}\n\n[workflow.job.matrix]\naxes = {{ runner = [\"{POOL}\"] }}\n\n[[workflow.job.step]]\nname = \"Build\"\nrun = \"echo ok\"\n"
        ),
        format!(
            "\n[[workflow.job]]\nid = \"build\"\nrunner = \"${{{{ matrix.runner }}}}\"\n\n[workflow.job.matrix]\naxes = {{ runner = [\"macos-15\", \"{POOL}\"] }}\n\n[[workflow.job.step]]\nname = \"Build\"\nrun = \"echo ok\"\n"
        ),
        format!(
            "\n[[workflow.job]]\nid = \"build\"\nrunner = \"${{{{ matrix.runner }}}}\"\n\n[workflow.job.matrix]\ninclude = [{{ runner = \"{POOL}\" }}]\n\n[[workflow.job.step]]\nname = \"Build\"\nrun = \"echo ok\"\n"
        ),
    ] {
        let message = refusal(&workflow(HEADER, ON, &jobs))?;
        assert!(message.contains("trusted_runner"), "{jobs}: {message}");
    }
    Ok(())
}

const GROUP: &str =
    "[[test_groups]]\nname = 'database'\nfilter = 'package(package-0) and test(/^db_/)'\n";

fn refused(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    pin_helper(&mut repo, "0.1.110")?;
    Ok(rust_workflow(&repo)
        .err()
        .ok_or_else(|| format!("accepted: {config}"))?
        .to_string())
}

#[test]
fn a_test_group_cannot_run_on_the_trusted_label() -> TestResult {
    for group in [
        format!("{GROUP}runner = '{POOL}'\n"),
        format!("{GROUP}runner = '{}'\n", POOL.to_uppercase()),
        format!("{GROUP}runner = '{POOL}'\nhosted = true\n"),
        format!("{GROUP}runner = '{POOL}'\nhosted = false\n"),
    ] {
        let config = format!("runner = 'velnor-box'\ntrusted_runner = '{POOL}'\n{group}");
        let error = refused(&config)?;
        assert!(error.contains("trusted_runner"), "{config}: {error}");
    }
    Ok(())
}

#[test]
fn a_test_group_on_its_own_label_leaves_the_trusted_runner_valid() -> TestResult {
    let config = format!(
        "runner = 'velnor-box'\ntrusted_runner = '{POOL}'\n{GROUP}runner = 'database-box'\n"
    );
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    pin_helper(&mut repo, "0.1.110")?;
    rust_workflow(&repo)?;
    Ok(())
}
