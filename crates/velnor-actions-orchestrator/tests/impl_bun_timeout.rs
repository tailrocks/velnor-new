//! Bun member job timeouts: member, then stack, then crate bound.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_bun::{MISE, bun_config, write_workspace};
use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};

fn member_timeouts(config: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    write_workspace(repo.path(), MISE)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    let minutes = |id: &str| -> Result<String, Box<dyn std::error::Error>> {
        let body = job_body(ci, id)?;
        Ok(body
            .lines()
            .find_map(|line| line.trim().strip_prefix("timeout-minutes: "))
            .ok_or("timeout-minutes")?
            .to_owned())
    };
    Ok((
        minutes("bun-chainargos-docs")?,
        minutes("bun-chainargos-ui")?,
    ))
}

#[test]
fn member_timeout_overrides_the_stack_and_crate_bounds_for_that_member_only() -> TestResult {
    let crate_default = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\ncrate_timeout_minutes = 12\n\
[stacks.bun]\nroots = [\"frontend\"]\n"
    );
    assert_eq!(member_timeouts(&crate_default)?, ("12".into(), "12".into()));
    let stack = bun_config("timeout_minutes = 30\n");
    assert_eq!(member_timeouts(&stack)?, ("30".into(), "30".into()));
    let member = bun_config(
        "timeout_minutes = 30\n[stacks.bun.members.\"chainargos-docs\"]\ntimeout_minutes = 90\n",
    );
    assert_eq!(member_timeouts(&member)?, ("90".into(), "30".into()));
    let member_over_crate =
        format!("{crate_default}[stacks.bun.members.\"@chainargos/ui\"]\ntimeout_minutes = 360\n");
    assert_eq!(
        member_timeouts(&member_over_crate)?,
        ("12".into(), "360".into())
    );
    Ok(())
}

#[test]
fn member_timeout_out_of_range_stops_generation() -> TestResult {
    for minutes in [0, 361] {
        let repo = make_repo(&bun_config(&format!(
            "[stacks.bun.members.\"chainargos-docs\"]\ntimeout_minutes = {minutes}\n"
        )))?;
        write_workspace(repo.path(), MISE)?;
        let error = prepare(repo.path()).expect_err("out of range");
        assert!(
            error
                .to_string()
                .contains(&format!("bad_timeout:{minutes}:expected_1..=360")),
            "{error}"
        );
    }
    Ok(())
}
