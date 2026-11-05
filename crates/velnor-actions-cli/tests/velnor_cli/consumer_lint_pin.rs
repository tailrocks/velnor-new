// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::fixture;

const CONSUMER_JACTIONLINT: &str = "9.9.9";

#[test]
fn staged_lint_runs_the_jactionlint_the_consumer_pins() -> Result<(), Box<dyn Error>> {
    let fixture = fixture::Fixture::new()?;
    fixture.pin_jactionlint(CONSUMER_JACTIONLINT)?;
    let output = fixture.root.join("generated");
    let generated = fixture.generate_unguarded(&output)?;
    assert!(
        generated.status.success(),
        "generation must lint with the consumer's pin: {}",
        fixture::stderr(&generated)
    );
    assert!(
        output
            .join(".github/workflows/binary-preview.yml")
            .is_file(),
        "preview workflow missing after staged lint"
    );
    assert_eq!(
        fixture.jactionlint_specs_run()?,
        [format!("jactionlint@{CONSUMER_JACTIONLINT}")],
        "staged lint ran a jactionlint other than the consumer's pin"
    );
    Ok(())
}
