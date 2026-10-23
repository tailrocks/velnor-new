use velnor_actions_mise::catalog::PinnedTool;
use velnor_actions_mise::toolfiles::lockfile::{
    InstallCoverage, audit_install_coverage, parse_mise_lockfile,
};

use super::{catalog, subject};

#[test]
fn added_platform_records_verify_and_missing_record_blocks() -> Result<(), String> {
    let lock = parse_mise_lockfile(include_str!("../../../mise.lock"))?;
    let catalog = catalog();
    let subjects = [
        subject(PinnedTool::Nextest, &catalog)?,
        subject(PinnedTool::MrBoxington, &catalog)?,
    ];

    for platform in [
        "linux-arm64",
        "linux-arm64-musl",
        "linux-x64-musl",
        "windows-x64",
    ] {
        let verdicts = audit_install_coverage(&lock, &subjects, platform);
        assert_eq!(
            verdicts,
            vec![InstallCoverage::Verified; subjects.len()],
            "both newly locked tools must verify on {platform}"
        );
    }

    let mut missing_record = lock.clone();
    let nextest = subjects.first().ok_or("Nextest subject")?;
    let nextest_key = &nextest.lock_key;
    let entries = missing_record
        .entries
        .get_mut(nextest_key)
        .ok_or("Nextest lock entries")?;
    let mut removed = false;
    for entry in entries {
        removed |= entry.checksums.remove("windows-x64").is_some();
    }
    assert!(removed, "test must remove an existing required record");
    if let Some(entry) = missing_record.tools.get_mut(nextest_key) {
        entry.checksums.remove("windows-x64");
    }

    let verdicts = audit_install_coverage(
        &missing_record,
        std::slice::from_ref(nextest),
        "windows-x64",
    );
    let verdict = verdicts.first().ok_or("Nextest coverage verdict")?;
    assert!(
        matches!(verdict, InstallCoverage::MissingPlatform { .. }),
        "removing a required lock row must fail closed: {verdict:?}"
    );
    assert!(verdict.is_blocking());
    Ok(())
}
