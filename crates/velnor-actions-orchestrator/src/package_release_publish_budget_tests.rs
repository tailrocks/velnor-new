use super::{parse_timeout, with_config, with_deadline, with_default_scope};
use std::{cell::Cell, ffi::OsString, time::Duration};
use velnor_actions_contract::workflow::JobTimeout;
use velnor_actions_mise::CheckDeadline;

#[test]
fn timeout_defaults_and_accepts_canonical_job_timeout_values() {
    assert_eq!(parse_timeout(None).expect("default"), JobTimeout::DEFAULT);
    for minutes in [1, 5, 15, 360] {
        assert_eq!(
            parse_timeout(Some(minutes.to_string().into()))
                .expect("canonical timeout")
                .minutes(),
            minutes
        );
    }
}

#[test]
fn timeout_rejects_noncanonical_and_out_of_range_values_without_leaking_input() {
    for value in ["", "0", "01", "+15", " 15", "15 ", "361", "65536", "١٥"] {
        let error = parse_timeout(Some(value.into())).expect_err("reject timeout");
        if !value.is_empty() {
            assert!(!format!("{error:?}").contains(value), "{error:?}");
        }
    }
}

#[cfg(unix)]
#[test]
fn timeout_rejects_non_unicode_input() {
    use std::os::unix::ffi::OsStringExt;
    assert!(parse_timeout(Some(OsString::from_vec(vec![b'1', 0xff]))).is_err());
}

#[test]
fn invalid_timeout_stops_before_the_publication_operation_runs() {
    let called = Cell::new(false);
    let result = with_config(Some("015".into()), std::time::Instant::now(), |_| {
        called.set(true);
        Ok(())
    });
    assert!(result.is_err());
    assert!(!called.get());
}

#[test]
fn configured_deadline_exceeds_five_minutes_and_is_not_restarted_by_subscopes() {
    let result = with_config(Some("15".into()), std::time::Instant::now(), |deadline| {
        let before = deadline.remaining().expect("publication time remains");
        assert!(before > Duration::from_mins(5));
        with_default_scope(|local| {
            assert!(local.remaining().expect("local time remains") <= Duration::from_mins(5));
            Ok(())
        })?;
        let after = deadline.remaining().expect("publication time remains");
        assert!(after <= before);
        Ok(())
    });
    assert!(result.is_ok());
}

#[test]
fn nested_timeout_restores_its_default_admission_parent() {
    let result = with_default_scope(|_| {
        let nested = CheckDeadline::after(Duration::from_millis(10)).expect("nested");
        let nested_result = nested.scope(|| std::thread::sleep(Duration::from_millis(15)));
        assert!(nested_result.is_err());
        assert!(CheckDeadline::check_scope().is_ok());
        Ok(())
    });
    assert!(result.is_ok());
}

#[test]
fn shorter_parent_limits_the_default_admission_scope() {
    let parent = CheckDeadline::after(Duration::from_millis(20)).expect("short parent");
    let result = with_deadline(parent, |_| {
        with_default_scope(|_| {
            std::thread::sleep(Duration::from_millis(35));
            Ok(())
        })
    });
    assert!(result.is_err());
}
