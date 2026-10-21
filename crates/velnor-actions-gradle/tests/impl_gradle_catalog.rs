use velnor_actions_gradle::catalog::*;

#[test]
fn rows_are_well_formed_and_ascending() {
    let versions: Vec<&str> = RELEASES.iter().map(|row| row.version).collect();
    let mut sorted = versions.clone();
    sorted.sort_by_key(|v| {
        v.split('.')
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    });
    assert_eq!(versions, sorted);
    for row in RELEASES {
        for digest in [row.bin_sha256, row.all_sha256, row.wrapper_jar_sha256] {
            assert_eq!(digest.len(), 64);
            assert!(digest.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
        }
        assert_ne!(row.bin_sha256, row.all_sha256);
    }
    assert!(release("9.5.1").is_some());
    assert!(release("9.5.1-rc-1").is_none());
}

#[test]
fn every_final_release_through_9_8_2_is_cataloged() {
    // Final releases from services.gradle.org/versions/all, 2026-10-21.
    for version in [
        "9.3.1", "9.4.1", "9.5.0", "9.5.1", "9.6.0", "9.6.1", "9.7.0", "9.7.1", "9.8.0", "9.8.1",
        "9.8.2",
    ] {
        assert!(
            release(version).is_some(),
            "The catalog must contain version {version}."
        );
    }
    assert_eq!(RELEASES.last().map(|row| row.version), Some("9.8.2"));
}

#[test]
fn gradle_9_8_2_digests_match_services_gradle_org() {
    let row = release("9.8.2").expect("The catalog must contain version 9.8.2.");
    assert_eq!(
        row.bin_sha256,
        "21bd5da2de49b642904077f38c0efdc7778e92b3ed0889392dca61b97e389607"
    );
    assert_eq!(
        row.wrapper_jar_sha256,
        "3b8a25775a69158b5ad2b1d17a80a88dc7b40a352a73eb27d06c612a7ce68e98"
    );
    assert!(is_official_wrapper_jar(row.wrapper_jar_sha256));
}
