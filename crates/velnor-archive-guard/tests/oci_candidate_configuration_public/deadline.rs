// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    cell::Cell,
    error::Error,
    fs,
    rc::Rc,
    time::{Duration, Instant},
};

use velnor_actions_mise::CheckDeadline;
use velnor_archive_guard::owned_archive_guard::oci_candidate::{
    OciCandidateEvidence, inspect_oci_candidate, materialize_oci_candidate,
};

use super::support;

#[test]
fn expired_deadlines_stop_inspection_and_materialization_before_side_effects()
-> Result<(), Box<dyn Error>> {
    let fixture = support::fixture(r#"{"User":"runner"}"#, false)?;
    let source = fs::read(&fixture.path)?;
    let scratch = tempfile::tempdir()?;
    let destination = scratch.path().join("candidate");
    let start = Instant::now();
    let deadline = CheckDeadline::from_start(start, Duration::from_secs(1))?;
    let inspection_started = Rc::new(Cell::new(false));
    let inspection_observed = Rc::clone(&inspection_started);
    let inspection_path = fixture.path.clone();
    let inspection = CheckDeadline::with_clock(
        move || start + Duration::from_secs(2),
        || {
            deadline.scope(|| {
                inspection_observed.set(true);
                inspect_oci_candidate(&inspection_path)
            })
        },
    );

    assert!(inspection.is_err());
    assert!(!inspection_started.get());
    let materialization_started = Rc::new(Cell::new(false));
    let materialization_observed = Rc::clone(&materialization_started);
    let materialization_path = fixture.path.clone();
    let materialization = CheckDeadline::with_clock(
        move || start + Duration::from_secs(2),
        || {
            deadline.scope(|| {
                materialization_observed.set(true);
                let expected = inspect_oci_candidate(&materialization_path)?;
                materialize_oci_candidate(&materialization_path, &destination, &expected)
            })
        },
    );

    assert!(materialization.is_err());
    assert!(!materialization_started.get());
    assert!(!destination.exists());
    assert_eq!(fs::read(&fixture.path)?, source);
    Ok(())
}

#[test]
fn unexpired_deadline_preserves_inspection_evidence_and_projection() -> Result<(), Box<dyn Error>> {
    let fixture = support::fixture(r#"{"User":"runner"}"#, false)?;
    let source = support::archive_files(&fixture.path)?;
    let scratch = tempfile::tempdir()?;
    let destination = scratch.path().join("candidate");
    let start = Instant::now();
    let deadline = CheckDeadline::from_start(start, Duration::from_secs(30))?;
    let result = CheckDeadline::with_clock(
        move || start,
        || {
            deadline.scope(
                || -> Result<(OciCandidateEvidence, OciCandidateEvidence), Box<dyn Error>> {
                    let inspected = inspect_oci_candidate(&fixture.path)?;
                    let materialized =
                        materialize_oci_candidate(&fixture.path, &destination, &inspected)?;
                    assert_eq!(super::projection_files(&destination)?, source);
                    Ok((inspected, materialized))
                },
            )
        },
    );

    let (inspected, materialized) = result??;
    assert_eq!(inspected, materialized);
    Ok(())
}

#[test]
fn timeout_during_projection_removes_only_the_owned_destination() -> Result<(), Box<dyn Error>> {
    let fixture = support::fixture(r#"{"User":"runner"}"#, false)?;
    let expected = inspect_oci_candidate(&fixture.path)?;
    let source = fs::read(&fixture.path)?;
    let scratch = tempfile::tempdir()?;
    let destination = scratch.path().join("candidate");
    let foreign = scratch.path().join("foreign-sentinel");
    fs::write(&foreign, b"preserve")?;
    let sentinel = fs::read(&foreign)?;
    let blob_name = support::archive_files(&fixture.path)?
        .into_iter()
        .find(|(name, bytes)| name.starts_with("blobs/sha256/") && !bytes.is_empty())
        .map(|(name, _)| name)
        .ok_or("fixture has no nonempty OCI blob")?;
    let projected_blob = destination.join(blob_name);
    let expired = Rc::new(Cell::new(false));
    let clock_expired = Rc::clone(&expired);
    let start = Instant::now();
    let deadline = CheckDeadline::from_start(start, Duration::from_secs(1))?;
    let mut materialized = None;
    let result = CheckDeadline::with_clock(
        move || {
            if !clock_expired.get()
                && fs::metadata(&projected_blob).is_ok_and(|metadata| metadata.len() > 0)
            {
                clock_expired.set(true);
            }
            if clock_expired.get() {
                start + Duration::from_secs(2)
            } else {
                start
            }
        },
        || {
            deadline.scope(|| {
                materialized = Some(materialize_oci_candidate(
                    &fixture.path,
                    &destination,
                    &expected,
                ));
            })
        },
    );

    assert!(result.is_err());
    assert!(expired.get());
    assert!(!destination.exists());
    assert!(matches!(materialized, Some(Err(ref error)) if error.is_deadline_exceeded()));
    assert_eq!(fs::read(&fixture.path)?, source);
    assert_eq!(fs::read(&foreign)?, sentinel);
    Ok(())
}
