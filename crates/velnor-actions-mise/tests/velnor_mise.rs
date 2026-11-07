// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod checkout_history_head_reads;
#[cfg(unix)]
mod command_stream;
mod gc1_t11;
mod gc1_t17;
mod gc1_t18;
mod install_spec_subject;
mod metadata_cwd;
#[cfg(unix)]
mod prepared_runner_stdin;
mod release_runner_catalog;
mod rust_spec_kind;
mod source_preview_lock;

#[test]
fn job_build_command_binds_validated_source_revision() -> Result<(), Box<dyn std::error::Error>> {
    let source = "a".repeat(40);
    let command = velnor_actions_mise::ImageCandidateCommand::build_oci(
        std::path::Path::new("/tmp/velnor-image-context"),
        std::path::Path::new("/tmp/velnor-image.tar"),
        std::path::Path::new("/tmp/velnor-docker-config"),
        &source,
    )?;
    let argv = command.argv();
    let expected_argument = format!("VELNOR_SOURCE_SHA={source}");
    assert!(argv.windows(2).any(|arguments| {
        arguments[0] == "--build-arg" && arguments[1].to_str() == Some(expected_argument.as_str())
    }));
    assert!(
        velnor_actions_mise::ImageCandidateCommand::build_oci(
            std::path::Path::new("/tmp/velnor-image-context"),
            std::path::Path::new("/tmp/velnor-image.tar"),
            std::path::Path::new("/tmp/velnor-docker-config"),
            "not-a-source-sha",
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn effective_deadline_retains_parent_after_scope_exit() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        cell::Cell,
        rc::Rc,
        time::{Duration, Instant},
    };
    use velnor_actions_mise::CheckDeadline;
    let anchor = Instant::now();
    let clock = Rc::new(Cell::new(anchor));
    let observed = Rc::clone(&clock);
    CheckDeadline::with_clock(
        move || observed.get(),
        || {
            let parent = CheckDeadline::from_start(anchor, Duration::from_secs(10))?;
            let unbounded = CheckDeadline::after(Duration::from_secs(600))?;
            let retained = parent.scope(|| {
                CheckDeadline::after(Duration::from_secs(600)).map(CheckDeadline::effective)
            })??;
            assert!(retained.remaining().is_ok());
            clock.set(anchor + Duration::from_secs(11));
            assert!(
                retained.remaining().is_err(),
                "captured deadline extended after parent scope exited"
            );
            assert!(unbounded.remaining().is_ok());
            Ok(())
        },
    )
}
