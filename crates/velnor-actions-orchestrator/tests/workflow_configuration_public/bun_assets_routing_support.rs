// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;

use super::support::rendered_job_block;

pub(super) fn configure_mixed_profiles(
    root: &std::path::Path,
    default_profile: &str,
    suite_profile: &str,
) -> Result<(), Box<dyn Error>> {
    let config_path = root.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?;
    let marker = "default_profile = \"hosted\"";
    assert_eq!(config.matches(marker).count(), 1);
    let mut config = config.replace(marker, &format!("default_profile = \"{default_profile}\""));
    write!(
        config,
        r#"

[execution.overrides."bun-fixture-web--unit"]
profile = "{suite_profile}"
role = "verification"

[execution.overrides."bun-fixture-web--browser"]
profile = "{suite_profile}"
role = "verification"
"#
    )?;
    fs::write(config_path, config)?;
    Ok(())
}

pub(super) fn mixed_rendered_job_ids(workflow: &str) -> BTreeSet<String> {
    workflow
        .lines()
        .skip_while(|line| *line != "jobs:")
        .skip(1)
        .filter_map(|line| line.strip_prefix("  "))
        .filter_map(|line| line.strip_suffix(':'))
        .filter(|id| !id.starts_with(' '))
        .map(str::to_owned)
        .collect()
}

pub(super) fn assert_mixed_report_artifacts(workflow: &str) -> Result<(), Box<dyn Error>> {
    let suite_id = "bun-fixture-web--browser";
    let suite = rendered_job_block(workflow, suite_id)?;
    let report = rendered_job_block(workflow, "bun-fixture-web--browser--report")?;
    assert!(suite.contains(&format!(
        "name: velnor-pw-blob-{suite_id}-leg-${{{{ matrix.leg }}}}"
    )));
    assert!(suite.contains(&format!(
        "name: velnor-pw-results-{suite_id}-leg-${{{{ matrix.leg }}}}"
    )));
    assert!(report.contains(&format!("pattern: velnor-pw-blob-{suite_id}-leg-*")));
    assert!(report.contains("name: velnor-pw-report-bun-fixture-web--browser"));
    Ok(())
}
