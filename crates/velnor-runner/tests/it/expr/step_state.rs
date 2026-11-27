// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use velnor_runner::expr::{expression_reads_step_state, template_reads_step_state};

use crate::expr_support::empty_env;

#[test]
fn a_template_reads_step_state_only_through_steps_env_status_and_files() -> Result<()> {
    let env = empty_env();
    let cases = [
        ("plain text", false),
        ("${{ github.sha }}", false),
        (
            "${{ runner.os }}-${{ needs.a.result }}-${{ inputs.x }}-${{ secrets.S }}",
            false,
        ),
        ("${{ format('{0}', toJSON(github.event)) }}", false),
        ("${{ always() }}", false),
        ("${{ steps.a.outputs.b }}", true),
        ("${{ env.X }}", true),
        ("${{ hashFiles('**/x') }}", true),
        ("${{ success() }}", true),
        ("${{ cancelled() }}", true),
        ("${{ failure() }}", true),
        ("a-${{ github.sha }}-${{ env.X }}", true),
        ("${{ format('{0}', steps.a.outputs.b) }}", true),
        ("${{ github.sha || env.X }}", true),
        ("${{ github.sha && steps.a.outputs.b }}", true),
        ("${{ !cancelled() }}", true),
        ("${{ github.sha == env.X }}", true),
        ("${{ github[env.K] }}", true),
    ];
    for (source, expected) in cases {
        assert_eq!(
            template_reads_step_state(source, &env)?,
            expected,
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn an_expression_reads_step_state_by_the_same_rule() -> Result<()> {
    let env = empty_env();
    for (source, expected) in [
        ("", false),
        ("github.sha", false),
        ("format('{0}', runner.os)", false),
        ("steps.a.outputs.b", true),
        ("env.X || 'y'", true),
        ("hashFiles('x')", true),
    ] {
        assert_eq!(
            expression_reads_step_state(source, &env)?,
            expected,
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn an_unparsable_template_or_an_unknown_context_is_an_error() {
    let env = empty_env();
    assert!(template_reads_step_state("${{ github.sha", &env).is_err());
    assert!(template_reads_step_state("${{ matrix.os }}", &env).is_err());
    assert!(expression_reads_step_state("matrix.os", &env).is_err());
}
