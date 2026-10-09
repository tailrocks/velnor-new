//! Source authority and strict time parser regression cases.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::context::{FreshnessContext, RUST_SOURCE_CAP, parse_iso_date, parse_timestamp};

const SOURCE_FIXTURE_ATTEMPTS: usize = 32;
static NEXT_SOURCE_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

fn source_fixture_root(process_id: u32, suffix: u128) -> PathBuf {
    std::env::temp_dir().join(format!("velnor-freshness-source-{process_id}-{suffix}"))
}

fn source_fixture_with_ids(
    source: &[u8],
    process_id: u32,
    mut next_id: impl FnMut() -> Option<u128>,
) -> Result<(PathBuf, FreshnessContext), String> {
    for _ in 0..SOURCE_FIXTURE_ATTEMPTS {
        let suffix = next_id().ok_or_else(|| "fixture identifier exhausted".to_owned())?;
        let root = source_fixture_root(process_id, suffix);
        match fs::create_dir(&root) {
            Ok(()) => {
                let pin_path = root.join("pins.rs");
                if let Err(write_error) = fs::write(&pin_path, source) {
                    let failure = format!(
                        "fixture write failed for {}: {write_error}",
                        pin_path.display()
                    );
                    return match fs::remove_dir_all(&root) {
                        Ok(()) => Err(failure),
                        Err(cleanup_error) => Err(format!(
                            "{failure}; fixture cleanup failed for {}: {cleanup_error}",
                            root.display()
                        )),
                    };
                }
                return Ok((root.clone(), FreshnessContext::new(root, false, false)));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(format!(
                    "fixture directory creation failed for {}: {error}",
                    root.display()
                ));
            }
        }
    }
    Err(format!(
        "fixture allocation exhausted {SOURCE_FIXTURE_ATTEMPTS} attempts"
    ))
}

fn source_fixture(source: &[u8]) -> Result<(PathBuf, FreshnessContext), String> {
    source_fixture_with_ids(source, std::process::id(), || {
        NEXT_SOURCE_FIXTURE_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .ok()
            .map(u128::from)
    })
}

fn remove_source_fixture(root: &Path) -> Result<(), String> {
    fs::remove_dir_all(root)
        .map_err(|error| format!("fixture cleanup failed for {}: {error}", root.display()))
}

fn check_source(source: &[u8]) -> (Option<String>, Vec<String>) {
    let (root, mut context) = match source_fixture(source) {
        Ok(fixture) => fixture,
        Err(error) => return (None, vec![error]),
    };
    let value = context.rust_const("pins.rs", "PIN");
    let failures = context.failures.clone();
    let mut failures = failures;
    if let Err(error) = remove_source_fixture(&root) {
        failures.push(error);
    }
    (value, failures)
}

#[test]
fn source_fixture_retries_stale_pid_scoped_paths() {
    let clock = SystemTime::now().duration_since(UNIX_EPOCH);
    assert!(clock.is_ok(), "system clock precedes UNIX epoch: {clock:?}");
    let suffix = clock.map_or_else(|_| 0, |duration| duration.as_nanos());
    let next_suffix = suffix.saturating_add(1);
    assert_ne!(next_suffix, suffix, "fixture suffix overflowed");
    let process_id = u32::MAX;
    let stale_root = source_fixture_root(process_id, suffix);
    let fresh_root = source_fixture_root(process_id, next_suffix);
    let stale_creation = fs::create_dir(&stale_root);
    assert!(stale_creation.is_ok(), "{stale_creation:?}");
    if stale_creation.is_err() {
        return;
    }
    let sentinel_path = stale_root.join("sentinel");
    let sentinel_write = fs::write(&sentinel_path, b"stale fixture");
    assert!(sentinel_write.is_ok(), "{sentinel_write:?}");
    if sentinel_write.is_err() {
        let cleanup = remove_source_fixture(&stale_root);
        assert!(cleanup.is_ok(), "{cleanup:?}");
        return;
    }

    let source = b"pub const PIN: &str = \"fresh\";\n";
    let mut suffixes = [Some(suffix), Some(next_suffix)].into_iter();
    let fixture = source_fixture_with_ids(source, process_id, || suffixes.next().flatten());
    assert!(fixture.is_ok(), "{fixture:?}");
    if let Ok((root, mut context)) = fixture {
        assert_eq!(root, fresh_root);
        assert_eq!(
            fs::read(root.join("pins.rs")).ok().as_deref(),
            Some(source.as_slice())
        );
        assert_eq!(
            context.rust_const("pins.rs", "PIN"),
            Some("fresh".to_owned())
        );
        assert!(context.failures.is_empty(), "{:?}", context.failures);
        let cleanup = remove_source_fixture(&root);
        assert!(cleanup.is_ok(), "{cleanup:?}");
    }
    assert_eq!(
        fs::read(&sentinel_path).ok().as_deref(),
        Some(b"stale fixture".as_slice())
    );
    let cleanup = remove_source_fixture(&stale_root);
    assert!(cleanup.is_ok(), "{cleanup:?}");
}

#[test]
fn concurrent_source_fixtures_keep_distinct_complete_sources() {
    const WORKERS: usize = 8;
    let workers = (0..WORKERS)
        .map(|index| {
            std::thread::spawn(move || -> Result<(PathBuf, String), String> {
                let expected = format!("pin-{index}");
                let source = format!("pub const PIN: &str = \"{expected}\";\n");
                let (root, mut context) = source_fixture(source.as_bytes())?;
                let pin = context.rust_const("pins.rs", "PIN");
                let mut failures = std::mem::take(&mut context.failures);
                if let Err(error) = remove_source_fixture(&root) {
                    failures.push(error);
                }
                if !failures.is_empty() {
                    return Err(failures.join("; "));
                }
                if pin != Some(expected.clone()) {
                    return Err(format!("fixture {} returned {pin:?}", root.display()));
                }
                Ok((root, expected))
            })
        })
        .collect::<Vec<_>>();

    let mut roots = HashSet::new();
    let mut failures = Vec::new();
    for worker in workers {
        match worker.join() {
            Ok(Ok((root, _))) => {
                if !roots.insert(root.clone()) {
                    failures.push(format!("duplicate fixture root: {}", root.display()));
                }
            }
            Ok(Err(error)) => failures.push(error),
            Err(_) => failures.push("fixture worker panicked".to_owned()),
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(roots.len(), WORKERS);
}

#[test]
fn literal_authority_accepts_only_one_unconditional_public_str_constant() {
    let source = b"pub const PIN: &str = \"v1.2.3\";\n";
    assert_eq!(
        check_source(source),
        (Some("v1.2.3".to_owned()), Vec::new())
    );
    assert!(check_source(b"const PIN: &str = \"private\";").0.is_none());
    assert!(
        check_source(b"pub const PIN: &String = \"wrong\";")
            .0
            .is_none()
    );
    assert!(
        check_source(b"pub const PIN: &str = concat!(\"v\", \"1\");")
            .0
            .is_none()
    );
    assert!(
        check_source(b"pub mod nested { pub const PIN: &str = \"hidden\"; }")
            .0
            .is_none()
    );
    assert!(
        check_source(b"impl Thing { pub const PIN: &str = \"associated\"; }")
            .0
            .is_none()
    );
}

#[test]
fn literal_authority_ignores_comments_and_nested_token_scopes() {
    let source = br####"
// pub const PIN: &str = "line comment decoy";
/* nested /* pub const PIN: &str = "block comment decoy"; */ comment */
const NOTE: &str = "pub const PIN: &str = \"string decoy\";";
const RAW: &str = r###"pub const PIN: &str = "raw decoy";"###;
macro_rules! fake { () => { pub const PIN: &str = "macro decoy"; } }
mod nested { pub const PIN: &str = "nested decoy"; }
struct Holder;
impl Holder { pub const PIN: &str = "associated decoy"; }
/// Documentation comments do not change the compiled pin.
pub const PIN: &str = "v1.2.3";
"####;
    assert_eq!(check_source(source).0, Some("v1.2.3".to_owned()));
}

#[test]
fn conditional_crate_attrs_duplicates_and_constant_attrs_fail_closed() {
    for source in [
        b"#![cfg(feature = \"alternate\")] pub const PIN: &str = \"v1\";".as_slice(),
        b"#![cfg_attr(feature = \"alternate\", cfg(test))] pub const PIN: &str = \"v1\";",
        b"pub const PIN: &str = \"one\"; pub const PIN: &str = \"two\";",
        b"#[cfg(feature = \"alternate\")] pub const PIN: &str = \"v1\";",
    ] {
        assert!(
            check_source(source).0.is_none(),
            "source accepted: {}",
            String::from_utf8_lossy(source)
        );
    }
}

#[test]
fn conditional_and_duplicate_pin_authorities_fail_closed() {
    for source in [
        b"#[cfg_attr(feature = \"alternate\", cfg(test))] pub const PIN: &str = \"v1\";".as_slice(),
        b"pub const PIN: &str = \"one\"; pub const PIN: &str = \"two\";",
        b"macro_rules! fake { () => { pub const PIN: &str = \"macro\"; } }",
        b"pub const PIN: &str = concat!(\"v\", \"1\");",
        b"pub const PIN: &String = \"wrong type\";",
        b"const PIN: &str = \"private\";",
        b"#[deprecated] pub const PIN: &str = \"attribute\";",
    ] {
        let (pin, failures) = check_source(source);
        assert!(
            pin.is_none() && !failures.is_empty(),
            "source accepted: {}",
            String::from_utf8_lossy(source)
        );
    }
}

#[test]
fn escaped_and_raw_string_pins_decode_to_compiled_values() {
    for source in [
        b"pub const PIN: &str = \"1\\x2e2.3\";".as_slice(),
        b"pub const PIN: &str = r#\"1.2.3\"#;",
    ] {
        let (pin, failures) = check_source(source);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(pin, Some("1.2.3".to_owned()));
    }
    let (pin, failures) = check_source(b"pub const PIN: &str = concat!(\"1\", \".2.3\");");
    assert!(pin.is_none());
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("must use one string literal"))
    );
}

#[test]
fn literal_parser_enforces_utf8_and_source_cap() {
    assert!(check_source(b"\xff").1[0].contains("unsupported Rust source"));
    let prefix = b"pub const PIN: &str = \"v1\";";
    let mut exact = prefix.to_vec();
    exact.resize(RUST_SOURCE_CAP, b' ');
    assert_eq!(check_source(&exact).0, Some("v1".to_owned()));
    exact.push(b' ');
    assert!(check_source(&exact).1[0].contains("source exceeds"));
}

#[test]
fn strict_dates_and_timezone_timestamps_are_normalized() {
    assert!(parse_iso_date("2026-02-29").is_none());
    assert!(parse_iso_date("+026-10-05").is_none());
    assert!(parse_iso_date("2024-02-29").is_some());
    assert_eq!(
        parse_timestamp("2026-10-05T12:00:00+02:00"),
        parse_timestamp("2026-10-05T10:00:00Z")
    );
    assert_eq!(
        parse_timestamp("2026-10-05T10:00:00.1234Z"),
        parse_timestamp("2026-10-05T10:00:00z")
    );
    assert_eq!(
        parse_timestamp("2026-10-05T10:00:00-0130"),
        parse_timestamp("2026-10-05T11:30:00Z")
    );
    assert!(parse_timestamp("2026-10-05T99:00:00Z").is_none());
    for invalid in [
        "2026-10-05T-1:00:00Z",
        "2026-10-05T00:00:00.garbageZ",
        "2026-10-05T00:00:00.Z",
        "2026-10-05T00:00:00.1.2Z",
        "2026-10-05T00:00:00+01:0x",
        "2026-10-05T00:00:00+24:00",
        "2026-10-05T00:00:00",
        " 2026-10-05T00:00:00Z",
        "2026-10-05T00:00:00.garbageZ",
        "2026-10-05T-9223372036854775808:00:00Z",
        "2026-10-05T00:00:00-9223372036854775808:00:00Z",
    ] {
        assert!(parse_timestamp(invalid).is_none(), "accepted {invalid:?}");
    }
}
