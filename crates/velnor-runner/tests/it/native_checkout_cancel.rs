// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, finish};
use crate::native_checkout_env::wrapper_env;
use crate::native_checkout_walk::warm;

const RUNS: usize = 10;
const ALWAYS_STEP: &str = r#"}
  ,
    {
      "Type": "action",
      "Id": "00000000-0000-0000-0000-000000000009",
      "Name": "__run",
      "ContextName": "__run",
      "Reference": { "Type": "script" },
      "DisplayNameToken": { "type": 0, "lit": "Probe" },
      "Inputs": {
        "type": 2,
        "map": [
          {
            "key": { "type": 0, "lit": "script" },
            "value": { "type": 0, "lit": "sleep 1; if kill -0 $(cat ../helper.pid) 2>/dev/null; then echo HELPER-ALIVE; else echo HELPER-GONE; fi" }
          }
        ]
      },
      "Condition": "always()"
    }
  ],
  "Variables""#;

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn a_cancelled_checkout_leaves_no_process_of_its_git_for_the_next_step() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    wrapper_env(
        &rig.root,
        "case \" $* \" in *\" fetch \"*--depth=3*) sh -c 'echo $$ > ../helper.pid; exec sleep 120' & sleep 120; exit 1;; esac\n",
    )?;
    let job = rig.job_edit(
        "checkout_depth2",
        &tip,
        &rig.server_url(),
        &[
            ("\"lit\": \"2\"", "\"lit\": \"3\""),
            ("}\n  ],\n  \"Variables\"", ALWAYS_STEP),
        ],
    )?;
    let scratches: Vec<Scratch> = (0..RUNS).map(|_| Scratch::new()).collect::<Result<_>>()?;
    let runners: Vec<_> = scratches
        .iter()
        .map(|scratch| rig.start(&job, scratch))
        .collect::<Result<_>>()?;
    for (scratch, runner) in scratches.iter().zip(&runners) {
        wait_for(&scratch.workspace().with_file_name("helper.pid"));
        std::thread::sleep(Duration::from_millis(300));
        Command::new("kill")
            .args(["-TERM", &runner.id().to_string()])
            .status()?;
    }
    let mut survivors = 0;
    for (scratch, runner) in scratches.iter().zip(runners) {
        let ran = finish(runner)?;
        if ran.stdout.contains("HELPER-ALIVE") {
            survivors += 1;
            let helper = std::fs::read_to_string(scratch.workspace().with_file_name("helper.pid"))?;
            Command::new("kill")
                .args(["-KILL", helper.trim()])
                .status()?;
        } else {
            assert!(ran.stdout.contains("HELPER-GONE"), "{}", ran.stdout);
        }
    }
    assert_eq!(
        survivors, 0,
        "{survivors} of {RUNS} helpers outlived the cancel"
    );
    Ok(())
}
