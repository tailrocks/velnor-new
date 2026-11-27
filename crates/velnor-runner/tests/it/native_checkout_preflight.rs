// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::exec_support::{Ran, Scratch};
use crate::native_checkout::{Rig, finish, git};
use crate::native_checkout_inputs::MARKER_STEP;

const AFTER_PERSIST: &str = "\"lit\": \"false\"\n            }\n          }";

fn persist_pair(name: &str, value: &str) -> String {
    format!(
        "{{ \"key\": {{ \"type\": 0, \"lit\": \"{name}\" }}, \"value\": {{ \"type\": 0, \"lit\": \"{value}\" }} }}"
    )
}

fn ran_step_one(ran: &Ran) -> bool {
    ran.stdout.contains("step-one-marker") || ran.stderr.contains("step-one-marker")
}

#[test]
fn the_last_declaration_of_an_input_decides_whatever_its_case() -> Result<()> {
    let rig = Rig::new()?;
    let later_true = format!(
        "\"lit\": \"false\"\n            }}\n          }},\n          {}",
        persist_pair("PERSIST-CREDENTIALS", "true")
    );
    let job = rig.job_edit(
        "checkout_depth1",
        &rig.head,
        &rig.server_url(),
        &[("\"Steps\": [", MARKER_STEP), (AFTER_PERSIST, &later_true)],
    )?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        ran.has_line(
            "unsupported: actions/checkout requires persist-credentials: false in velnor-runner"
        ),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(!ran_step_one(&ran), "step 1 ran");
    assert!(!rig.trace().exists(), "git ran");
    Ok(())
}

#[test]
fn a_later_declaration_in_another_case_may_restore_persist_credentials_false() -> Result<()> {
    let rig = Rig::new()?;
    let later_false = format!(
        "\"lit\": \"true\"\n            }}\n          }},\n          {}",
        persist_pair("PERSIST-CREDENTIALS", "false")
    );
    let job = rig.job_edit(
        "checkout_depth1",
        &rig.head,
        &rig.server_url(),
        &[(AFTER_PERSIST, &later_false)],
    )?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    Ok(())
}

#[test]
fn a_job_whose_facts_cannot_be_built_is_refused_before_its_first_step() -> Result<()> {
    let rig = Rig::new()?;
    let job = rig.job_edit(
        "checkout_depth1",
        &rig.head,
        &rig.server_url(),
        &[
            ("\"Steps\": [", MARKER_STEP),
            ("\"k\": \"repository_id\"", "\"k\": \"renamed_id\""),
        ],
    )?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        ran.has_line("unsupported: trusted facts: repository_id is missing"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(!ran_step_one(&ran), "step 1 ran");
    assert!(!rig.trace().exists(), "git ran");
    Ok(())
}
