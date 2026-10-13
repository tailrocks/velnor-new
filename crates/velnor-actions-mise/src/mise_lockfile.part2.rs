/// Coverage of one install: entry match, version match, CI checksum shape.
///
/// Looks up the subject's lock key only: any other key's entry is
/// invisible to this install and must not affect its verdict.
fn coverage_for_subject(
    lock: &MiseLockfile,
    subject: &InstallSubject,
    platform: &str,
) -> InstallCoverage {
    let Some(entry) = lock.tools.get(&subject.lock_key) else {
        return InstallCoverage::MissingEntry;
    };
    if entry.version != subject.expected_version {
        return InstallCoverage::VersionDrift {
            locked: entry.version.clone(),
        };
    }
    match entry.checksums.get(platform) {
        Some(checksum) if is_enforceable_checksum(checksum) => InstallCoverage::Verified,
        Some(checksum) => InstallCoverage::CorruptChecksum {
            observed: checksum.clone(),
        },
        None if entry.checksums.is_empty() => InstallCoverage::NoChecksums,
        None => InstallCoverage::MissingPlatform {
            locked_platforms: entry.checksums.keys().cloned().collect(),
        },
    }
}
