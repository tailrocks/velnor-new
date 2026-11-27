// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};

use crate::mise_double::{each_row, prepared};
use crate::mise_world::job_fixture;

type Pairs<'a> = Vec<(&'a str, &'a str)>;

#[test]
fn mise_action_takes_cargo_and_rustup_homes_only_from_the_store() -> Result<()> {
    each_row(&["job message", "process env", "neither"], |row| {
        let world = prepared()?;
        let (job_env, process_env): (Pairs<'_>, Pairs<'_>) = match row {
            "job message" => (
                vec![("CARGO_HOME", "/c"), ("RUSTUP_HOME", "/r"), ("HOME", "/h")],
                vec![],
            ),
            "process env" => (vec![], vec![("CARGO_HOME", "/pc"), ("RUSTUP_HOME", "/pr")]),
            _ => (vec![], vec![]),
        };
        let job = world.with_job_env(&job_fixture("mise_action_ok"), &job_env)?;
        let ran = world.run(&job, &process_env)?;
        assert!(ran.has_line("job succeeded"), "{row}\n{}", ran.stdout);
        let tools = world.store.join("tools");
        let calls = world.calls();
        assert_eq!(calls.len(), 5, "{row}");
        for call in calls {
            let label = format!("{row} {}", call.argv);
            assert_eq!(
                call.env_value("MISE_CARGO_HOME"),
                tools.join("cargo").display().to_string(),
                "{label}"
            );
            assert_eq!(
                call.env_value("MISE_RUSTUP_HOME"),
                tools.join("rustup").display().to_string(),
                "{label}"
            );
            for name in ["CARGO_HOME", "RUSTUP_HOME"] {
                assert!(!call.env.contains_key(name), "{label}: {name} leaked");
            }
        }
        Ok(())
    })
}

#[test]
fn mise_action_rewrites_only_a_leading_private_prefix() -> Result<()> {
    let world = prepared()?;
    world.configure_double(&[("EMBEDDED", "1"), ("NO_PATHS", "1")])?;
    let job = world.with_final_script(
        &job_fixture("mise_action_ok"),
        "echo \"lead=$LEAD\"\necho \"emb=$EMB\"\necho \"sib=$SIB\"\necho \"$PATH\" | tr ':' '\\n' | sed 's/^/pathentry=/'",
    )?;
    let ran = world.run(&job, &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    let workspace = world.workspace().display().to_string();
    let private = world
        .calls()
        .first()
        .context("the recorded calls")?
        .pwd
        .display()
        .to_string();
    for expected in [
        format!("lead={workspace}/ok"),
        format!("emb=pre{private}-post"),
        format!("sib={private}2/x"),
        format!("pathentry={workspace}/bin"),
        format!("pathentry={private}2/bin"),
    ] {
        assert!(ran.has_line(&expected), "{expected}\n{}", ran.stdout);
    }
    assert!(
        !ran.stdout.contains(&format!("{workspace}2")),
        "{}",
        ran.stdout
    );
    Ok(())
}
