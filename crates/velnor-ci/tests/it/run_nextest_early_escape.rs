// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use crate::run_nextest::listed;
use crate::run_nextest_early_bound::{
    GRACE_VARIABLE, PATIENT, RUN_END, RUN_START, assert_withheld, finish, gone, launch, list_pid,
    wait_for,
};
use std::time::{Duration, Instant};

const HOLDER: &str = r#"/usr/bin/perl -MPOSIX -e 'open(my $f, ">", "$ENV{RUNNER_TEMP}/list-pid") or die; print $f "$$\n"; close($f) or die; POSIX::setsid() or die; exec @ARGV' /bin/sh -c ': > "$RUNNER_TEMP/escaped"; exec sleep 300'"#;
const WAIT_FOR_ESCAPE: &str = "i=0\nwhile [ ! -e \"$RUNNER_TEMP/escaped\" ] && [ $i -lt 100 ]; do sleep 0.05; i=$((i+1)); done\n";
const DRAINED: &str = "velnor-ci: the early nextest list failed: cargo-nextest exited, but a process it started left its process group and still held its output 2000 ms later";

#[test]
fn a_list_whose_descendant_leaves_its_process_group_holding_its_output_returns_a_grace_after_it_exits()
 {
    let json = listed(&[("kept", &["alpha"])]);
    for (held, redirect) in [("stdout", "2>/dev/null"), ("stderr", ">/dev/null")] {
        let mut run = launch(
            &format!("early-escape-{held}"),
            &format!(
                "echo printed-before-exit >&2\n{HOLDER} {redirect} &\n{WAIT_FOR_ESCAPE}printf '%s\\n' '{json}'\n: > \"$RUNNER_TEMP/list-exited\"\nexit 0"
            ),
            &format!("{RUN_START}{RUN_END}"),
            &[(GRACE_VARIABLE, PATIENT), ("CARGO_HOME", "cargo-home")],
        );
        let temp = run.temp.path().to_path_buf();
        wait_for(&temp.join("list-exited"));
        let anchor = Instant::now();
        let (exited, stderr, written) = finish(&mut run, Duration::from_secs(10));
        assert!(
            temp.join("escaped").exists() && temp.join("list-exited").exists(),
            "{held}: the holder never escaped or the list never ended: {stderr}"
        );
        let holder = list_pid(&temp).unwrap_or_else(|| unreachable!("{held}: no holder pid"));
        assert!(
            !gone(holder),
            "{held}: the holder died, so nothing held the pipe"
        );
        assert!(
            exited,
            "{held}: velnor-ci waited on a list that had exited while an escaped process held its {held}: {stderr}"
        );
        assert!(
            run.child
                .try_wait()
                .ok()
                .flatten()
                .is_some_and(|status| status.success()),
            "{held}: {stderr}"
        );
        assert!(
            anchor.elapsed() >= Duration::from_millis(1500),
            "{held}: velnor-ci gave up on the output {:?} after the list exited",
            anchor.elapsed()
        );
        assert!(written.contains("built=true\n"), "{held}: {written}");
        assert_withheld(&written);
        assert!(stderr.contains(DRAINED), "{held}: {stderr}");
        assert!(stderr.contains("printed-before-exit"), "{held}: {stderr}");
    }
}
