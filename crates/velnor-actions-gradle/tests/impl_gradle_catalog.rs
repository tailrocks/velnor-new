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
