// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::prepare::{DEPENDENCY_DEBUG, Setup, digest, prepare, reported};

#[test]
fn prepare_sets_the_build_profile_and_writes_a_compat_digest_when_every_tool_is_at_its_pin() {
    let run = prepare("ok", &Setup::new(reported("1.99.1", Some("0.9.151"))));
    assert!(run.output.status.success(), "{:?}", run.output);
    assert_eq!(
        run.env_file,
        "RUSTUP_AUTO_INSTALL=0\nCARGO_INCREMENTAL=0\nCARGO_TERM_COLOR=always\nSOURCE_DATE_EPOCH=0\nCARGO_PROFILE_DEV_DEBUG=line-tables-only\nCARGO_PROFILE_TEST_DEBUG=line-tables-only\n"
    );
    assert_eq!(digest(&run).len(), 16, "{}", run.outputs);
    assert!(digest(&run).bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(run.cargo_config, "");
    assert!(
        run.velnor_config.ends_with(DEPENDENCY_DEBUG),
        "{}",
        run.velnor_config
    );
}

#[test]
fn compat_is_stable_and_follows_rustflags_and_the_debuginfo_setting() {
    let json = reported("1.99.1", Some("0.9.151"));
    let first = prepare("stable", &Setup::new(json.clone()));
    let again = prepare("stable", &Setup::new(json.clone()));
    assert_eq!(digest(&first), digest(&again));
    let mut flags = Setup::new(json.clone());
    flags.env.push(("RUSTFLAGS", "-C target-cpu=native"));
    assert_ne!(digest(&first), digest(&prepare("stable", &flags)));
    let mut full = Setup::new(json);
    full.rust_ci_toml = Some("ci_debuginfo = \"full\"\n");
    let full = prepare("stable", &full);
    assert_ne!(digest(&first), digest(&full));
    assert!(full.env_file.contains("CARGO_PROFILE_DEV_DEBUG=full\n"));
    assert_eq!(
        (full.cargo_config.as_str(), full.velnor_config.as_str()),
        ("", "")
    );
}

#[test]
fn the_compat_digest_of_the_plain_fixture_is_pinned() {
    let run = prepare("pinned", &Setup::new(reported("1.99.1", Some("0.9.151"))));
    assert_eq!(digest(&run), "b29bb827d63005a2");
}
